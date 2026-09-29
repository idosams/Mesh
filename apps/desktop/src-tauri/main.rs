//! Native Tauri host for the local Mesh workspace management window.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(unix))]
compile_error!("The Mesh desktop MVP currently requires the daemon's Unix-domain socket transport");

#[cfg(unix)]
mod active_workspace;

#[cfg(unix)]
mod build_identity;

#[cfg(unix)]
mod attachment_capture;

#[cfg(unix)]
mod attachment_host;
mod attachment_recovery;
#[cfg(unix)]
mod fleet_host;
#[cfg(unix)]
mod worker_installation;
#[cfg(target_os = "macos")]
mod worker_service;

#[cfg(unix)]
mod artifact_preview;

#[cfg(unix)]
mod review_inspection;

#[cfg(unix)]
mod codex_workspace;
#[cfg(unix)]
mod desktop_attention;

#[cfg(unix)]
mod native_capture_preference;

#[cfg(unix)]
mod recent_workspace;

#[cfg(unix)]
mod renderer_proof;

#[cfg(unix)]
mod version_workspace;

#[cfg(unix)]
mod desktop {
    use std::ffi::{CStr, CString};
    use std::fs;
    use std::io::{self, BufRead as _, BufReader, Read as _, Seek as _, Write as _};
    use std::os::fd::{AsRawFd as _, FromRawFd as _, RawFd};
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[cfg(target_os = "macos")]
    use core_foundation::{base::TCFType as _, url::CFURL};
    use mesh_approval::{
        ApprovalDecision, Blake3 as ApprovalBlake3, ContentDigest as _, ExpectedHumanApproval,
        HumanApprovalReceiptDraft, APPROVAL_ALGORITHM, APPROVAL_APPLICATION_SCOPE,
        APPROVAL_USER_VERIFICATION,
    };
    use mesh_crypto::KeyCustody as _;
    use mesh_daemon::ipc::{
        nothing_to_recover, ChunkAssembler, ClientMessage, DaemonMessage, IpcServer, Json,
        Operations, ServerHandle, StartupSummary, WorkspaceSummary, PROTOCOL, SURFACE_VERSION,
    };
    use mesh_daemon::{
        CheckpointRuntimeParameters, LiveDaemon, ManagedContentDigest, ManagedDirectoryExport,
        ManagedEntryChange, ManagedFileExportPreview, ManagedTextFileError, ProtectedWorkspaceRoot,
        VerifiedManagedWorkspacePath, WorkspaceVersionForkRequest,
    };
    use mesh_keychain::{
        fresh_approval_challenge, SecureEnclaveApprovalCredential, SecureEnclaveApprovalError,
        SoftwareActorCustody,
    };
    use tauri::{Manager as _, State};
    use tauri_plugin_dialog::{DialogExt as _, MessageDialogButtons};

    use crate::active_workspace::ActiveWorkspaceLink;
    use crate::artifact_preview::{encode_base64, inspection_extension, render as render_artifact};
    use crate::attachment_host::AttachmentHost;
    #[cfg(test)]
    use crate::codex_workspace::ensure_codex_project_config;
    use crate::codex_workspace::{
        codex_launch_overrides, ensure_codex_project_config_at_references, CodexProjectConfig,
        CodexProjectConfigError,
    };
    use crate::desktop_attention::{request_existing_desktop_attention, DesktopAttentionServer};
    use crate::native_capture_preference::NativeCapturePreference;
    use crate::recent_workspace::{RecentWorkspace, RecentWorkspaceEntry};
    use crate::renderer_proof::RendererProofRuntime;
    use crate::review_inspection::{
        export_review_inspection, prune_app_owned_review_inspections, ReviewInspectionCopy,
    };
    use crate::version_workspace::VersionWorkspaceDirectory;

    const DESKTOP_SESSION: &str = "mesh-desktop";
    const DAEMON_REFUSAL_KIND: &str = "mesh-daemon-refusal";
    const DAEMON_REPLY_TIMEOUT: Duration = Duration::from_secs(10);
    // Opening or refreshing a workspace re-verifies its complete native inventory and projects a
    // bounded but potentially multi-megabyte state reply. These calls are read-only and safe to
    // leave in flight; a short interactive socket deadline only converts healthy large folders
    // into false recovery failures during Refresh and Finish agent handoff.
    const DAEMON_WORKSPACE_READ_REPLY_TIMEOUT: Duration = Duration::from_secs(5 * 60);
    // A confirmed import hashes, copies, re-verifies and durably ingests every included file.
    // Reusing the ordinary interactive deadline here turns a healthy large import into an
    // ambiguous timeout, and the renderer's one permitted recovery replay can then allocate and
    // ingest a duplicate workspace while the first request is still running. Keep this bound
    // beyond the UI's explicit 15-minute progress wait so the UI, rather than the socket, owns the
    // visible timeout without launching a second native transaction.
    const DAEMON_IMPORT_REPLY_TIMEOUT: Duration = Duration::from_secs(20 * 60);
    const MAX_LOCAL_ENDPOINT_BYTES: usize = 100;
    const MAX_RETAINED_REVIEW_INSPECTIONS: usize = 32;
    const MAX_WORKSPACE_EDITOR_TEXT_BYTES: usize = 1_048_576;
    const MAX_RENDERER_SCREENSHOT_BYTES: u64 = 16 * 1_024 * 1_024;
    const DAEMON_SOCKET_NAME: &str = "daemon.sock";

    #[cfg(feature = "git-integration")]
    type GitSetupDestinationIdentity = mesh_git_bridge::GitDestinationIdentity;
    #[cfg(not(feature = "git-integration"))]
    type GitSetupDestinationIdentity = ();

    #[cfg(test)]
    thread_local! {
        static BEFORE_PREPARED_GIT_INSTALL: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
            std::cell::RefCell::new(None);
    }

    fn daemon_refusal(code: &str, message: &str) -> String {
        Json::object([
            ("kind", Json::text(DAEMON_REFUSAL_KIND)),
            ("code", Json::text(code)),
            ("message", Json::text(message)),
        ])
        .encode()
    }

    fn encode_hex(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write as _;
            write!(&mut out, "{byte:02x}").expect("writing to a String cannot fail");
        }
        out
    }

    struct DesktopRuntime {
        endpoint: PathBuf,
        daemon: Arc<LiveDaemon>,
        author: Mutex<SoftwareActorCustody>,
        active_workspace: ActiveWorkspaceLink,
        version_workspaces: VersionWorkspaceDirectory,
        native_capture: NativeCapturePreference,
        recent: RecentWorkspace,
        recent_status: Mutex<RecentWorkspaceStatus>,
        renderer_proof: RendererProofRuntime,
        _server: Mutex<Option<ServerHandle>>,
    }

    enum DesktopEndpoint {
        Owned(IpcServer),
        AlreadyOwned,
    }

    fn bind_desktop_endpoint(endpoint: &Path) -> io::Result<DesktopEndpoint> {
        match IpcServer::bind(endpoint) {
            Ok(server) => Ok(DesktopEndpoint::Owned(server)),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
                Ok(DesktopEndpoint::AlreadyOwned)
            }
            Err(error) => Err(error),
        }
    }

    /// Choose a stable owner-specific runtime directory whose socket fits every supported Unix
    /// `sockaddr_un`.
    ///
    /// Tauri's application-data directory normally gives the desktop a private, stable owner. A
    /// long account name or redirected home can make `<app-data>/runtime/daemon.sock` exceed
    /// macOS's 104-byte `sun_path`, though. Letting `bind` discover that limit turns an otherwise
    /// healthy first launch into a non-unwinding Tauri setup panic. The short fallback is namespaced
    /// by the application-data owner's numeric user ID and is still secured to mode 0700 by
    /// `IpcServer::bind`; it carries only the local sockets, never workspace or navigation data.
    fn desktop_runtime_directory(app_data_dir: &Path) -> io::Result<PathBuf> {
        let preferred = app_data_dir.join("runtime");
        if preferred.join(DAEMON_SOCKET_NAME).as_os_str().len() <= MAX_LOCAL_ENDPOINT_BYTES {
            return Ok(preferred);
        }

        fs::create_dir_all(app_data_dir)?;
        let owner =
            <fs::Metadata as std::os::unix::fs::MetadataExt>::uid(&fs::metadata(app_data_dir)?);
        // `/tmp` exists on every Unix platform supported by the desktop. Canonicalizing it avoids
        // macOS's `/tmp` symlink while retaining the daemon's no-symlink endpoint-parent rule.
        let temporary_root = fs::canonicalize("/tmp")?;
        let fallback = temporary_root.join(format!("mesh-desktop-{owner}"));
        let endpoint = fallback.join(DAEMON_SOCKET_NAME);
        if endpoint.as_os_str().len() > MAX_LOCAL_ENDPOINT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "the shortest available local endpoint is {} bytes and exceeds the {MAX_LOCAL_ENDPOINT_BYTES}-byte platform limit",
                    endpoint.as_os_str().len()
                ),
            ));
        }
        Ok(fallback)
    }

    fn reveal_desktop_window(app: &tauri::AppHandle) {
        #[cfg(target_os = "macos")]
        let _ = app.show();
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }

    #[derive(Clone, Debug)]
    struct RecentWorkspaceNavigationEntry {
        path: String,
        export_root: Option<String>,
        project_root: Option<String>,
        agent_handoff_installation: Option<String>,
        agent_handoff_generation: Option<String>,
        source_point_ordinal: Option<u64>,
        original_update_version: Option<String>,
    }

    impl RecentWorkspaceNavigationEntry {
        fn from_entry(entry: &RecentWorkspaceEntry) -> Self {
            Self {
                path: entry.path().display().to_string(),
                export_root: entry.export_root().map(|root| root.display().to_string()),
                project_root: entry.project_root().map(|root| root.display().to_string()),
                agent_handoff_installation: entry.agent_handoff_installation().map(str::to_owned),
                agent_handoff_generation: entry.agent_handoff_generation().map(str::to_owned),
                source_point_ordinal: entry.source_point_ordinal(),
                original_update_version: entry.original_update_version().map(str::to_owned),
            }
        }

        fn to_json(&self) -> Json {
            Json::object([
                ("path", Json::text(&self.path)),
                (
                    "export_root",
                    self.export_root.as_ref().map_or(Json::Null, Json::text),
                ),
                (
                    "project_root",
                    self.project_root.as_ref().map_or(Json::Null, Json::text),
                ),
                (
                    "agent_handoff_installation",
                    self.agent_handoff_installation
                        .as_ref()
                        .map_or(Json::Null, Json::text),
                ),
                (
                    "agent_handoff_generation",
                    self.agent_handoff_generation
                        .as_ref()
                        .map_or(Json::Null, Json::text),
                ),
                (
                    "source_point_ordinal",
                    self.source_point_ordinal.map_or(Json::Null, Json::Number),
                ),
                (
                    "original_update_version",
                    self.original_update_version
                        .as_ref()
                        .map_or(Json::Null, Json::text),
                ),
            ])
        }
    }

    #[derive(Clone, Debug)]
    struct RecentWorkspaceStatus {
        remembered: Option<String>,
        workspaces: Vec<String>,
        workspace_entries: Vec<RecentWorkspaceNavigationEntry>,
        auto_opened: bool,
        active_folder: Option<String>,
        export_root: Option<String>,
        warning: Option<String>,
    }

    impl RecentWorkspaceStatus {
        fn empty() -> Self {
            Self {
                remembered: None,
                workspaces: Vec::new(),
                workspace_entries: Vec::new(),
                auto_opened: false,
                active_folder: None,
                export_root: None,
                warning: None,
            }
        }

        fn to_json(&self) -> String {
            Json::object([
                (
                    "remembered",
                    self.remembered.as_ref().map_or(Json::Null, Json::text),
                ),
                (
                    "workspaces",
                    Json::Array(self.workspaces.iter().map(Json::text).collect()),
                ),
                (
                    "workspace_entries",
                    Json::Array(
                        self.workspace_entries
                            .iter()
                            .map(RecentWorkspaceNavigationEntry::to_json)
                            .collect(),
                    ),
                ),
                ("auto_opened", Json::Bool(self.auto_opened)),
                (
                    "active_folder",
                    self.active_folder.as_ref().map_or(Json::Null, Json::text),
                ),
                (
                    "export_root",
                    self.export_root.as_ref().map_or(Json::Null, Json::text),
                ),
                (
                    "warning",
                    self.warning.as_ref().map_or(Json::Null, Json::text),
                ),
                ("build_revision", Json::text(env!("MESH_BUILD_REVISION"))),
                (
                    "build_exact",
                    Json::Bool(env!("MESH_BUILD_REVISION") != "development"),
                ),
            ])
            .encode()
        }
    }

    fn add_recent_warning(status: &mut RecentWorkspaceStatus, warning: String) {
        status.warning = Some(match status.warning.take() {
            Some(existing) => format!("{existing} {warning}"),
            None => warning,
        });
    }

    fn recent_navigation_entries(
        entries: &[RecentWorkspaceEntry],
    ) -> Vec<RecentWorkspaceNavigationEntry> {
        entries
            .iter()
            .map(RecentWorkspaceNavigationEntry::from_entry)
            .collect()
    }

    fn activate_verified_native_folder(
        active_workspace: &ActiveWorkspaceLink,
        verified: &VerifiedManagedWorkspacePath,
    ) -> Result<PathBuf, String> {
        if let Err(error) = verified.ensure_current() {
            let cleanup = active_workspace.deactivate_matching_target(verified.path());
            return Err(match cleanup {
                Ok(()) => format!(
                    "The native workspace changed before activation, so Mesh withdrew navigation only if it still named that workspace: {error}"
                ),
                Err(cleanup) => format!(
                    "The native workspace changed before activation: {error}. Mesh could not safely inspect its matching navigation link: {cleanup}"
                ),
            });
        }
        let activation = match active_workspace.activate_or_deactivate(verified.path()) {
            Ok(activation) => activation,
            Err(error) => {
                return Err(format!(
                    "The stable native folder could not follow the verified workspace. Mesh removed only navigation owned by this attempt and preserved any competing replacement: {error}"
                ));
            }
        };
        if let Err(error) = verified.ensure_current() {
            let cleanup = activation.deactivate_if_unchanged(active_workspace);
            return Err(match cleanup {
                Ok(()) => format!(
                    "The native workspace changed while its stable path was activated, so Mesh withdrew that exact navigation link: {error}"
                ),
                Err(cleanup) => format!(
                    "The native workspace changed while its stable path was activated: {error}. Mesh could not safely withdraw that exact navigation link: {cleanup}"
                ),
            });
        }
        Ok(activation.path().to_path_buf())
    }

    fn reconcile_active_native_folder(
        daemon: &LiveDaemon,
        active_workspace: &ActiveWorkspaceLink,
    ) -> Result<Option<PathBuf>, String> {
        match daemon.workspace_state() {
            Ok(open) => {
                let verified = daemon
                    .verified_managed_workspace_path(&open.root, &open.digest, &open.installation)
                    .map_err(|error| error.to_string())?;
                if verified.is_presented() {
                    activate_verified_native_folder(active_workspace, &verified).map(Some)
                } else {
                    active_workspace
                        .deactivate()
                        .map_err(|error| error.to_string())?;
                    Ok(None)
                }
            }
            Err(unavailable) if unavailable.code == "no-workspace-open" => {
                active_workspace
                    .deactivate()
                    .map_err(|error| error.to_string())?;
                Ok(None)
            }
            Err(unavailable) => {
                active_workspace
                    .deactivate()
                    .map_err(|error| error.to_string())?;
                Err(format!(
                    "The current workspace could not be verified, so its stable native folder was deactivated. {}",
                    unavailable.message
                ))
            }
        }
    }

    fn reconcile_verified_native_folder(
        daemon: &LiveDaemon,
        active_workspace: &ActiveWorkspaceLink,
        expected_workspace_root: &str,
        expected_workspace_digest: &str,
        expected_workspace_installation: &str,
    ) -> Result<(Option<PathBuf>, PathBuf), String> {
        let verified = daemon
            .verified_managed_workspace_path(
                expected_workspace_root,
                expected_workspace_digest,
                expected_workspace_installation,
            )
            .map_err(|error| error.to_string())?;
        let real = verified.path().to_path_buf();
        if verified.is_presented() {
            activate_verified_native_folder(active_workspace, &verified)
                .map(|stable| (Some(stable), real))
        } else {
            active_workspace
                .deactivate()
                .map_err(|error| error.to_string())?;
            Ok((None, real))
        }
    }

    fn forget_workspace_navigation(
        daemon: &LiveDaemon,
        recent: &RecentWorkspace,
        active_workspace: &ActiveWorkspaceLink,
        path: &std::path::Path,
    ) -> Result<RecentWorkspaceStatus, String> {
        // Rollback may ask to forget a workspace whose navigation hint the person already removed.
        // That is already the desired record state, but the stable path still has to be reconciled
        // against the daemon's actual post-action workspace.
        let _removed = recent
            .forget_if_matches(path)
            .map_err(|error| error.to_string())?;
        let entries = recent.load_entries().map_err(|error| error.to_string())?;
        let workspaces: Vec<_> = entries
            .iter()
            .map(|entry| entry.path().display().to_string())
            .collect();
        let open_root = daemon.workspace_state().ok().map(|open| open.root);
        let export_root = open_root.as_deref().and_then(|root| {
            entries
                .iter()
                .find(|entry| entry.path().to_str() == Some(root))
                .and_then(|entry| entry.export_root())
                .map(|path| path.display().to_string())
        });
        let (active_folder, warning) =
            match reconcile_active_native_folder(daemon, active_workspace) {
                Ok(active) => (active.map(|path| path.display().to_string()), None),
                Err(warning) => (None, Some(warning)),
            };
        Ok(RecentWorkspaceStatus {
            remembered: workspaces.first().cloned(),
            workspaces,
            workspace_entries: recent_navigation_entries(&entries),
            auto_opened: false,
            active_folder,
            export_root,
            warning,
        })
    }

    fn remember_current_workspace_navigation(
        daemon: &LiveDaemon,
        recent: &RecentWorkspace,
        active_workspace: &ActiveWorkspaceLink,
        open: &WorkspaceSummary,
        export_root: Option<&std::path::Path>,
    ) -> Result<RecentWorkspaceStatus, String> {
        remember_current_workspace_navigation_with_project_root(
            daemon,
            recent,
            active_workspace,
            open,
            export_root,
            None,
            None,
        )
    }

    fn remember_current_workspace_navigation_with_project_root(
        daemon: &LiveDaemon,
        recent: &RecentWorkspace,
        active_workspace: &ActiveWorkspaceLink,
        open: &WorkspaceSummary,
        export_root: Option<&std::path::Path>,
        inherited_project_root: Option<&std::path::Path>,
        source_point_ordinal: Option<u64>,
    ) -> Result<RecentWorkspaceStatus, String> {
        let verified = daemon
            .verified_managed_workspace_path(&open.root, &open.digest, &open.installation)
            .map_err(|error| error.to_string())?;
        let mut warnings = Vec::new();

        // The stable path is the live navigation surface used by Finder and following editors. Reconcile it
        // before persisting optional recent-history convenience, so a corrupt or unwritable history
        // record can never leave the old workspace named as current after a verified switch.
        let active_folder = if verified.is_presented() {
            match activate_verified_native_folder(active_workspace, &verified) {
                Ok(path) => Some(path.display().to_string()),
                Err(error) => {
                    warnings.push(error);
                    None
                }
            }
        } else {
            if let Err(error) = active_workspace.deactivate() {
                warnings.push(format!(
                    "The older workspace is open, but its prior stable-folder link needs attention: {error}"
                ));
            }
            None
        };

        let (workspaces, workspace_entries, export_root) = match recent
            .remember_repairing_invalid_content_at_point(
                verified.path(),
                export_root,
                inherited_project_root,
                source_point_ordinal,
            ) {
            Ok((_, repaired)) => {
                if repaired {
                    warnings.push(
                        "Mesh replaced malformed app-owned recent-workspace content. Older navigation hints could not be recovered, but the verified current workspace will reopen normally."
                            .to_owned(),
                    );
                }
                match recent.load_entries() {
                    Ok(entries) => {
                        let export_root = entries
                            .iter()
                            .find(|entry| entry.path() == verified.path())
                            .and_then(|entry| entry.export_root())
                            .map(|path| path.display().to_string());
                        let workspaces = entries
                            .iter()
                            .map(|entry| entry.path().display().to_string())
                            .collect();
                        (workspaces, recent_navigation_entries(&entries), export_root)
                    }
                    Err(error) => {
                        warnings.push(format!(
                        "The workspace is open, but Mesh could not read its recent-workspace history after saving it: {error}"
                    ));
                        (Vec::new(), Vec::new(), None)
                    }
                }
            }
            Err(error) => {
                let navigation = if active_folder.is_some() {
                    " and its stable native folder is current"
                } else {
                    ""
                };
                warnings.push(format!(
                    "The workspace is open{navigation}, but Mesh could not remember it for the next launch: {error}"
                ));
                (Vec::new(), Vec::new(), None)
            }
        };
        Ok(RecentWorkspaceStatus {
            remembered: workspaces.first().cloned(),
            workspaces,
            workspace_entries,
            auto_opened: false,
            active_folder,
            export_root,
            warning: (!warnings.is_empty()).then(|| warnings.join(" ")),
        })
    }

    fn reopen_remembered_workspace(
        daemon: &LiveDaemon,
        recent: &RecentWorkspace,
        active_workspace: &ActiveWorkspaceLink,
    ) -> RecentWorkspaceStatus {
        let mut status = RecentWorkspaceStatus::empty();
        match recent.load_entries() {
            Ok(entries) if !entries.is_empty() => {
                status.workspaces = entries
                    .iter()
                    .map(|entry| entry.path().display().to_string())
                    .collect();
                status.workspace_entries = recent_navigation_entries(&entries);
                let path = entries[0].path();
                status.remembered = Some(path.display().to_string());
                status.export_root = entries[0]
                    .export_root()
                    .map(|root| root.display().to_string());
                match daemon.reopen_at_start(path) {
                    Ok(opened) => match daemon.verified_managed_workspace_path(
                        &opened.root,
                        &opened.digest,
                        &opened.installation,
                    ) {
                        Ok(verified) => {
                            status.auto_opened = true;
                            if verified.is_presented() {
                                match activate_verified_native_folder(active_workspace, &verified) {
                                    Ok(active) => {
                                        status.active_folder = Some(active.display().to_string());
                                    }
                                    Err(error) => add_recent_warning(
                                        &mut status,
                                        format!(
                                            "The remembered workspace reopened, but its stable native folder could not be activated. {error}"
                                        ),
                                    ),
                                }
                            } else if let Err(error) = active_workspace.deactivate() {
                                add_recent_warning(
                                    &mut status,
                                    format!(
                                        "The older workspace reopened, but a prior stable-folder link needs attention. {error}"
                                    ),
                                );
                            }

                            // Successful reopen is also the promised migration point for the old
                            // one-path record. Re-publish only the daemon-verified canonical path;
                            // failure is a navigation warning and never invalidates the open workspace.
                            match recent.remember(verified.path()) {
                                Ok(_) => match recent.load_entries() {
                                    Ok(reopened) => {
                                        status.workspaces = reopened
                                            .iter()
                                            .map(|entry| entry.path().display().to_string())
                                            .collect();
                                        status.workspace_entries =
                                            recent_navigation_entries(&reopened);
                                        status.remembered = status.workspaces.first().cloned();
                                        status.export_root = reopened
                                            .first()
                                            .and_then(|entry| entry.export_root())
                                            .map(|root| root.display().to_string());
                                    }
                                    Err(error) => add_recent_warning(
                                        &mut status,
                                        format!(
                                            "The workspace reopened, but Mesh could not read its updated recent-workspace history: {error}"
                                        ),
                                    ),
                                },
                                Err(error) => add_recent_warning(
                                    &mut status,
                                    format!(
                                        "The workspace reopened, but Mesh could not update its recent-workspace history: {error}"
                                    ),
                                ),
                            }
                        }
                        Err(error) => {
                            status.warning = Some(format!(
                                "The remembered workspace reopened, but its native folder could not be verified. {error}"
                            ));
                            deactivate_failed_startup_navigation(active_workspace, &mut status);
                        }
                    },
                    Err(error) => {
                        status.warning = Some(format!(
                            "The remembered workspace could not be reopened safely. Choose it again to inspect the recovery details. ({})",
                            error.code()
                        ));
                        deactivate_failed_startup_navigation(active_workspace, &mut status);
                    }
                }
            }
            Ok(_) => {
                if let Err(error) = active_workspace.deactivate() {
                    status.warning = Some(format!(
                        "Mesh has no remembered workspace, but a prior stable-folder link needs attention. {error}"
                    ));
                }
            }
            Err(error) => {
                status.warning = Some(format!(
                    "Mesh refused its recent-workspace record. Choose the managed folder again. {error}"
                ));
                deactivate_failed_startup_navigation(active_workspace, &mut status);
            }
        }
        status
    }

    fn deactivate_failed_startup_navigation(
        active_workspace: &ActiveWorkspaceLink,
        status: &mut RecentWorkspaceStatus,
    ) {
        if let Err(cleanup) = active_workspace.deactivate() {
            add_recent_warning(
                status,
                format!("Its prior stable-folder link also needs attention. {cleanup}"),
            );
        }
    }

    #[tauri::command]
    async fn pick_folder(
        app: tauri::AppHandle,
        runtime: State<'_, DesktopRuntime>,
    ) -> Result<Option<String>, String> {
        if let Some(destination) = runtime
            .renderer_proof
            .take_private_export_picker_destination()?
        {
            // Keep the proof path on the same genuinely asynchronous reply boundary as the real
            // chooser. An immediately-ready branch inside this otherwise blocking async command
            // intermittently logged native acceptance without waking WKWebView's invoke promise.
            // The bounded proof value is already confined by RendererProofRuntime; moving only its
            // owned String through the blocking pool cannot grant filesystem or picker authority.
            return tauri::async_runtime::spawn_blocking(move || Some(destination))
                .await
                .map_err(|_| "The packaged folder-picker proof stopped unexpectedly".to_owned());
        }
        // The dialog plugin's blocking API is intended for asynchronous command contexts. A
        // synchronous Tauri command may execute on the application event thread; opening
        // NSOpenPanel from there previously made the macOS picker unsafe enough that the UI hid
        // it entirely. Keep the simple request/response contract, but let Tauri schedule this
        // command on its async runtime so the native event loop remains responsive.
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|path| {
                path.into_path()
                    .map(|value| value.to_string_lossy().into_owned())
                    .map_err(|error| error.to_string())
            })
            .transpose()
    }

    #[tauri::command]
    async fn attach_existing_project(
        host: State<'_, Arc<AttachmentHost>>,
        source: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.attach(Path::new(&source)))
            .await
            .map_err(|_| "Project attachment did not finish".to_owned())?
    }

    #[tauri::command]
    async fn attached_projects(host: State<'_, Arc<AttachmentHost>>) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.projects())
            .await
            .map_err(|_| "Attachment status is unavailable".to_owned())?
    }

    #[tauri::command]
    async fn attached_fleets(host: State<'_, Arc<AttachmentHost>>) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.fleets())
            .await
            .map_err(|_| "Fleet status is unavailable".to_owned())?
    }

    #[tauri::command]
    async fn remote_fleet_reviews(
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        after: u64,
        snapshot: Option<u64>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.remote_fleet_reviews(&objective, after, snapshot)
        })
        .await
        .map_err(|_| "Remote results could not be loaded".to_owned())?
    }
    #[tauri::command]
    async fn inspect_remote_fleet_review(
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        offer: String,
        correlation: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.remote_fleet_review(&objective, &offer, &correlation)
        })
        .await
        .map_err(|_| "Remote review could not be loaded".to_owned())?
    }
    #[allow(clippy::too_many_arguments)]
    #[tauri::command]
    async fn render_remote_fleet_artifact(
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        offer: String,
        correlation: String,
        object_id: String,
        side: String,
        page_number: Option<usize>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let artifact =
                host.remote_fleet_artifact(&objective, &offer, &correlation, &object_id, &side)?;
            let rendered = render_artifact(artifact.path(), artifact.bytes(), page_number)
                .map_err(|e| e.to_string())?;
            Ok(Json::object([
                ("schema", Json::text("mesh.remote-artifact-preview/v1")),
                ("objective", Json::text(objective)),
                ("offer", Json::text(offer)),
                ("correlation", Json::text(correlation)),
                ("object", Json::text(object_id)),
                (
                    "preview",
                    rendered_artifact_json(
                        &rendered,
                        &side,
                        &artifact.version().to_string(),
                        &artifact.digest().to_string(),
                    ),
                ),
            ])
            .encode())
        })
        .await
        .map_err(|_| "Remote artifact could not be rendered".to_owned())?
    }

    #[tauri::command]
    async fn fleet_saved_reviews(
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        lane: String,
        after: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.fleet_history(&objective)?
                .saved_reviews(&lane, after.as_deref())
                .map(|value| value.encode())
                .map_err(|_| "Saved fleet results are unavailable".into())
        })
        .await
        .map_err(|_| "Saved fleet results could not be loaded".to_owned())?
    }

    #[tauri::command]
    async fn inspect_fleet_saved_review(
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            host.fleet_history(&objective)?
                .saved_review(&selection)
                .map(|value| value.encode())
                .map_err(|_| "The exact saved review is unavailable".into())
        })
        .await
        .map_err(|_| "Saved fleet review could not be loaded".to_owned())?
    }

    #[allow(clippy::too_many_arguments)]
    #[tauri::command]
    async fn fleet_project_mapping(
        host: State<'_, Arc<AttachmentHost>>,
        project: String,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
        after: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            host.fleet_project_mapping(
                &project,
                &objective,
                &selection,
                &attachment_review_trust(),
                after.as_deref(),
            )
        })
        .await
        .map_err(|_| {
            "Project candidate operation did not finish; keep its exact inputs for retry".to_owned()
        })?
    }

    #[allow(clippy::too_many_arguments)]
    #[tauri::command]
    async fn prepare_fleet_project_candidate(
        host: State<'_, Arc<AttachmentHost>>,
        project: String,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
        request: String,
        expected_main: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            host.prepare_fleet_candidate(
                &project,
                &objective,
                &selection,
                &attachment_review_trust(),
                &request,
                expected_main.as_deref(),
            )
        })
        .await
        .map_err(|_| {
            "Project candidate operation did not finish; keep its exact inputs for retry".to_owned()
        })?
    }

    #[allow(clippy::too_many_arguments)]
    #[tauri::command]
    async fn import_fleet_project_candidate(
        host: State<'_, Arc<AttachmentHost>>,
        project: String,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
        request: String,
        expected_main: Option<String>,
        create: bool,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            host.import_fleet_candidate(
                &project,
                &objective,
                &selection,
                &attachment_review_trust(),
                &request,
                expected_main.as_deref(),
                create,
            )
        })
        .await
        .map_err(|_| {
            "The import operation did not finish; keep its exact inputs for retry".to_owned()
        })?
    }

    #[allow(clippy::too_many_arguments)]
    #[tauri::command]
    async fn review_imported_fleet_project_candidate(
        host: State<'_, Arc<AttachmentHost>>,
        project: String,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
        request: String,
        expected_main: Option<String>,
        create: bool,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            host.review_imported_fleet_candidate(
                &project,
                &objective,
                &selection,
                &attachment_review_trust(),
                &request,
                expected_main.as_deref(),
                create,
            )
        })
        .await
        .map_err(|_| {
            "The import operation did not finish; keep its exact inputs for retry".to_owned()
        })?
    }

    #[allow(clippy::too_many_arguments)]
    #[tauri::command]
    async fn review_fleet_project_candidate(
        host: State<'_, Arc<AttachmentHost>>,
        project: String,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
        request: String,
        expected_main: Option<String>,
        after: Option<String>,
        selected: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            host.review_fleet_candidate(
                &project,
                &objective,
                &selection,
                &attachment_review_trust(),
                &request,
                expected_main.as_deref(),
                (after.as_deref(), selected.as_deref()),
            )
        })
        .await
        .map_err(|_| {
            "Project candidate operation did not finish; keep its exact inputs for retry".to_owned()
        })?
    }

    #[allow(clippy::too_many_arguments)]
    #[tauri::command]
    async fn request_fleet_review_changes(
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
        request: String,
        message: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            let receipt = host
                .current_fleet(&objective)?
                .request_review_changes(&request, &selection, &message)
                .map_err(|_| "Review change request could not be recorded")?;
            Ok(Json::object([
                ("schema", Json::text("mesh.fleet-review-change-receipt/v1")),
                ("objective", Json::text(objective)),
                ("selection", selection.to_json()),
                ("request", Json::text(request)),
                ("change", receipt),
            ])
            .encode())
        })
        .await
        .map_err(|_| "Review change request did not finish".to_owned())?
    }

    #[allow(clippy::too_many_arguments)]
    #[tauri::command(async)]
    fn decide_fleet_review_change(
        app: tauri::AppHandle,
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
        request: String,
        operation: String,
        expected_revision: u64,
        proposed_checkpoint: Option<String>,
    ) -> Result<String, String> {
        let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
            &lane,
            &checkpoint,
            &version,
            &bundle,
        )
        .map_err(|_| "Saved review selection is invalid")?;
        let outcome = host.current_fleet(&objective)?.decide_review_change(&selection, &request, &operation,
            expected_revision, proposed_checkpoint.as_deref(), |prompt| app.dialog().message(prompt)
                .title("Change request decision")
                .buttons(MessageDialogButtons::OkCancelCustom("Confirm request decision".into(), "Cancel".into()))
                .blocking_show())
            .map_err(|_| "Request decision was not confirmed. Read its current state before choosing again, or retry the same operation to recover an existing receipt.")?;
        Ok(Json::object([
            ("schema", Json::text("mesh.fleet-review-decision/v1")),
            ("objective", Json::text(objective)),
            ("selection", selection.to_json()),
            ("operation", Json::text(operation)),
            ("request", Json::text(request)),
            ("outcome", outcome),
        ])
        .encode())
    }

    #[tauri::command]
    async fn fleet_review_changes(
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            let activity = host
                .fleet_history(&objective)?
                .saved_review_change_activity(&selection)
                .map_err(|_| "Saved review change requests are unavailable")?;
            Ok(Json::object([
                ("schema", Json::text("mesh.fleet-review-changes/v3")),
                ("objective", Json::text(objective)),
                ("selection", selection.to_json()),
                ("activity", activity),
            ])
            .encode())
        })
        .await
        .map_err(|_| "Review change requests could not be read".to_owned())?
    }

    // Only exact saved identities cross this boundary. Neither paths nor execution authority
    // are accepted, and restored history uses the same authenticated artifact reader.
    #[allow(clippy::too_many_arguments)]
    #[tauri::command]
    async fn render_fleet_review_artifact(
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
        object_id: String,
        side: String,
        page_number: Option<usize>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            let artifact = host
                .fleet_history(&objective)?
                .saved_review_artifact(&selection, &object_id, &side)
                .map_err(|_| "The exact saved artifact is unavailable")?;
            let rendered = render_artifact(artifact.path(), artifact.bytes(), page_number)
                .map_err(|error| error.to_string())?;
            Ok(Json::object([
                ("schema", Json::text("mesh.fleet-artifact-preview/v1")),
                ("objective", Json::text(objective)),
                ("selection", selection.to_json()),
                ("object", Json::text(object_id)),
                (
                    "preview",
                    rendered_artifact_json(
                        &rendered,
                        &side,
                        &artifact.version().to_string(),
                        &artifact.digest().to_string(),
                    ),
                ),
            ])
            .encode())
        })
        .await
        .map_err(|_| "Saved artifact rendering did not finish".to_owned())?
    }

    // Tauri binds the exact selection and bounded comparison selectors as named arguments.
    #[allow(clippy::too_many_arguments)]
    #[tauri::command]
    async fn inspect_fleet_starting_comparison(
        host: State<'_, Arc<AttachmentHost>>,
        objective: String,
        lane: String,
        checkpoint: String,
        version: String,
        bundle: String,
        after: Option<String>,
        selected: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let selection = mesh_daemon::fleet::service::SavedReviewSelection::new(
                &lane,
                &checkpoint,
                &version,
                &bundle,
            )
            .map_err(|_| "Saved review selection is invalid")?;
            host.fleet_history(&objective)?
                .saved_starting_comparison(&selection, after.as_deref(), selected.as_deref())
                .map(|value| value.encode())
                .map_err(|_| "The exact starting-version comparison is unavailable".into())
        })
        .await
        .map_err(|_| "Starting-version comparison could not be loaded".to_owned())?
    }

    #[tauri::command]
    async fn fleet_activity(
        hosts: State<'_, Arc<crate::fleet_host::FleetHosts>>,
    ) -> Result<String, String> {
        hosts.snapshot()
    }

    #[tauri::command]
    async fn start_attached_fleet(
        host: State<'_, Arc<AttachmentHost>>,
        hosts: State<'_, Arc<crate::fleet_host::FleetHosts>>,
        runtime: State<'_, DesktopRuntime>,
        objective: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        let hosts = Arc::clone(hosts.inner());
        let daemon = runtime.daemon.clone();
        let endpoint = runtime.endpoint.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let service = host.current_fleet(&objective)?;
            let bridge =
                std::env::current_exe().map_err(|_| "The Mesh executable is unavailable")?;
            let adapters = service
                .admitted_providers()
                .map(|provider| {
                    use mesh_daemon::fleet::provider::{
                        ClaudeAdapter, CodexAdapter, NativeAdapter,
                    };
                    match provider {
                        "codex" => CodexAdapter::with_desktop_bridge(
                            &codex_cli_path().ok_or("An installed Codex provider is required")?,
                            &bridge,
                        )
                        .map(NativeAdapter::from)
                        .map_err(|_| "The installed Codex provider could not be admitted"),
                        "claude" => ClaudeAdapter::with_desktop_bridge(
                            &claude_cli_path().ok_or("An installed Claude provider is required")?,
                            &bridge,
                        )
                        .map(NativeAdapter::from)
                        .map_err(|_| "The installed Claude provider could not be admitted"),
                        _ => Err("The saved provider policy is unsupported"),
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            hosts.start(service, &daemon, adapters, endpoint)?;
            hosts.snapshot()
        })
        .await
        .map_err(|_| "Fleet start needs reconciliation".to_owned())?
    }

    #[tauri::command]
    async fn stop_attached_fleet(
        host: State<'_, Arc<AttachmentHost>>,
        hosts: State<'_, Arc<crate::fleet_host::FleetHosts>>,
        objective: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        let hosts = Arc::clone(hosts.inner());
        tauri::async_runtime::spawn_blocking(move || {
            hosts.stop(&host.current_fleet(&objective)?)?;
            hosts.snapshot()
        })
        .await
        .map_err(|_| "Fleet stop needs reconciliation".to_owned())?
    }

    #[tauri::command]
    async fn provision_attached_fleet(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        request: String,
        goal: String,
        version: String,
        limits_json: String,
        policy_json: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || match policy_json.as_deref() {
            Some(policy) => host.provision_fleet_with_policy(
                &id,
                &request,
                &goal,
                &version,
                &limits_json,
                Some(policy),
            ),
            None => host.provision_fleet(&id, &request, &goal, &version, &limits_json),
        })
        .await
        .map_err(|_| "Fleet provisioning did not finish".to_owned())?
    }

    #[tauri::command]
    async fn attached_project_versions(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        before: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.versions(&id, before.as_deref()))
            .await
            .map_err(|_| "Attachment history is unavailable".to_owned())?
    }

    #[tauri::command]
    async fn inspect_attached_version(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        operation: String,
        path: Option<String>,
        after: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.inspect(&id, &operation, path.as_deref(), after.as_deref())
        })
        .await
        .map_err(|_| "Saved version inspection is unavailable".to_owned())?
    }

    #[tauri::command]
    async fn compare_attached_versions(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        base: String,
        target: String,
        after: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.compare(&id, &base, &target, after.as_deref())
        })
        .await
        .map_err(|_| "Saved version comparison is unavailable".to_owned())?
    }

    #[tauri::command]
    async fn request_attached_review(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        target: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.request_review(&id, &target, &attachment_review_trust())
        })
        .await
        .map_err(|_| "Review request stopped".to_owned())?
    }

    #[tauri::command]
    async fn attached_project_reviews(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.reviews(&id, &attachment_review_trust()))
            .await
            .map_err(|_| "Review listing stopped".to_owned())?
    }

    #[tauri::command]
    async fn inspect_attached_review(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        bundle: String,
        target: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.review(&id, &bundle, &target, &attachment_review_trust())
        })
        .await
        .map_err(|_| "Review inspection stopped".to_owned())?
    }

    #[tauri::command]
    async fn load_attachment_pins(host: State<'_, Arc<AttachmentHost>>) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.load_pins())
            .await
            .map_err(|_| "Pin loading stopped".to_owned())?
    }

    #[tauri::command]
    async fn save_attachment_pins(
        host: State<'_, Arc<AttachmentHost>>,
        snapshot: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.save_pins(&snapshot))
            .await
            .map_err(|_| "Pin saving stopped".to_owned())?
    }

    #[tauri::command]
    async fn load_fleet_review_outbox(
        host: State<'_, Arc<AttachmentHost>>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.load_fleet_review_outbox())
            .await
            .map_err(|_| "Pending review loading stopped".to_owned())?
    }
    #[tauri::command]
    async fn save_fleet_review_outbox(
        host: State<'_, Arc<AttachmentHost>>,
        snapshot: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.save_fleet_review_outbox(&snapshot))
            .await
            .map_err(|_| "Pending review saving stopped".to_owned())?
    }

    #[tauri::command]
    async fn load_fleet_pins(host: State<'_, Arc<AttachmentHost>>) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.load_fleet_pins())
            .await
            .map_err(|_| "Pin loading stopped".to_owned())?
    }

    #[tauri::command]
    async fn save_fleet_pins(
        host: State<'_, Arc<AttachmentHost>>,
        snapshot: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.save_fleet_pins(&snapshot))
            .await
            .map_err(|_| "Pin saving stopped".to_owned())?
    }

    #[tauri::command]
    async fn compare_attached_path(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        base: String,
        target: String,
        path: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.comparison_path(&id, &base, &target, &path)
        })
        .await
        .map_err(|_| "Pin preview stopped".to_owned())?
    }

    #[tauri::command]
    async fn control_attached_project(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        generation: String,
        action: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || host.control(&id, &generation, &action))
            .await
            .map_err(|_| "Attachment control did not finish".to_owned())?
    }

    #[tauri::command]
    fn recent_workspace_status(runtime: State<'_, DesktopRuntime>) -> Result<String, String> {
        // Another Mesh process may have acquired or released agent custody since this runtime
        // started. Reload that safety-critical slice before projecting it to the renderer.
        reconcile_current_agent_handoff(&runtime.daemon, &runtime.recent)?;
        refresh_agent_handoff_navigation(&runtime.recent, &runtime.recent_status)?;
        runtime
            .recent_status
            .lock()
            .map_err(|_| "The recent workspace status is unavailable".to_owned())
            .map(|status| status.to_json())
    }

    #[tauri::command]
    fn renderer_proof_configuration(runtime: State<'_, DesktopRuntime>) -> Result<String, String> {
        runtime.renderer_proof.configuration()
    }

    #[cfg(target_os = "macos")]
    const RENDERER_SCREENSHOT_OPEN_DIRECTORY_FLAGS: i32 = 0x0010_0000 | 0x0000_0100;
    #[cfg(target_os = "macos")]
    const RENDERER_SCREENSHOT_CREATE_FILE_FLAGS: i32 =
        0x0000_0002 | 0x0000_0200 | 0x0000_0800 | 0x0000_0100;

    #[cfg(target_os = "macos")]
    #[allow(unsafe_code)]
    fn renderer_screenshot_mkdirat(directory: RawFd, name: &CStr, mode: u32) -> io::Result<()> {
        unsafe extern "C" {
            fn mkdirat(directory: i32, path: *const std::ffi::c_char, mode: u32) -> i32;
        }
        // SAFETY: `name` is a live C string and `directory` is a retained directory descriptor.
        if unsafe { mkdirat(directory, name.as_ptr(), mode) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[allow(unsafe_code, clashing_extern_declarations)]
    fn renderer_screenshot_openat(
        directory: RawFd,
        name: &CStr,
        flags: i32,
        mode: i32,
    ) -> io::Result<fs::File> {
        unsafe extern "C" {
            #[link_name = "openat"]
            fn openat_with_mode(
                directory: i32,
                path: *const std::ffi::c_char,
                flags: i32,
                ...
            ) -> i32;
        }
        // SAFETY: `name` is a live C string and a successful call transfers one descriptor.
        let descriptor = unsafe { openat_with_mode(directory, name.as_ptr(), flags, mode) };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful `openat` returned a fresh owned descriptor.
        Ok(unsafe { fs::File::from_raw_fd(descriptor) })
    }

    #[cfg(target_os = "macos")]
    #[allow(unsafe_code)]
    fn renderer_screenshot_unlinkat(directory: RawFd, name: &CStr) -> io::Result<()> {
        unsafe extern "C" {
            fn unlinkat(directory: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
        }
        // SAFETY: `name` is a live C string and flags=0 removes only the named non-directory entry.
        if unsafe { unlinkat(directory, name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn renderer_screenshot_dimensions(png: &[u8]) -> Result<(u32, u32), String> {
        if png.len() < 256
            || png.len() as u64 > MAX_RENDERER_SCREENSHOT_BYTES
            || !png.starts_with(b"\x89PNG\r\n\x1a\n")
            || png.get(12..16) != Some(b"IHDR")
        {
            return Err("packaged Files screenshot was not a bounded PNG".to_owned());
        }
        let width = u32::from_be_bytes(
            png.get(16..20)
                .and_then(|bytes| bytes.try_into().ok())
                .ok_or_else(|| "packaged Files screenshot width was missing".to_owned())?,
        );
        let height = u32::from_be_bytes(
            png.get(20..24)
                .and_then(|bytes| bytes.try_into().ok())
                .ok_or_else(|| "packaged Files screenshot height was missing".to_owned())?,
        );
        if !(64..=8_192).contains(&width) || !(64..=8_192).contains(&height) {
            return Err("packaged Files screenshot dimensions were invalid".to_owned());
        }
        Ok((width, height))
    }

    #[cfg(target_os = "macos")]
    fn persist_renderer_files_screenshot(
        application_data: &Path,
        filename: &str,
        png: &[u8],
    ) -> Result<(PathBuf, u32, u32, u64, String), String> {
        let (width, height) = renderer_screenshot_dimensions(png)?;
        let expected_application_data = fs::symlink_metadata(application_data)
            .map_err(|_| "packaged Files screenshot storage was unavailable".to_owned())?;
        if !expected_application_data.is_dir()
            || expected_application_data.file_type().is_symlink()
            || expected_application_data.permissions().mode() & 0o077 != 0
        {
            return Err("packaged Files screenshot storage was not private".to_owned());
        }
        let application_data_directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(RENDERER_SCREENSHOT_OPEN_DIRECTORY_FLAGS)
            .open(application_data)
            .map_err(|_| "packaged Files screenshot storage was unavailable".to_owned())?;
        let actual_application_data = application_data_directory
            .metadata()
            .map_err(|_| "packaged Files screenshot storage was unavailable".to_owned())?;
        if actual_application_data.dev() != expected_application_data.dev()
            || actual_application_data.ino() != expected_application_data.ino()
            || !actual_application_data.is_dir()
        {
            return Err("packaged Files screenshot storage changed during capture".to_owned());
        }

        let root_name = CString::new("renderer-proof").expect("fixed screenshot directory name");
        match renderer_screenshot_mkdirat(application_data_directory.as_raw_fd(), &root_name, 0o700)
        {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                return Err("packaged Files screenshot storage could not be created".to_owned())
            }
        }
        let root_directory = renderer_screenshot_openat(
            application_data_directory.as_raw_fd(),
            &root_name,
            RENDERER_SCREENSHOT_OPEN_DIRECTORY_FLAGS,
            0,
        )
        .map_err(|_| "packaged Files screenshot storage was unavailable".to_owned())?;
        root_directory
            .set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(|_| "packaged Files screenshot storage was not private".to_owned())?;
        let root_metadata = root_directory
            .metadata()
            .map_err(|_| "packaged Files screenshot storage was unavailable".to_owned())?;
        if !root_metadata.is_dir() || root_metadata.permissions().mode() & 0o077 != 0 {
            return Err("packaged Files screenshot storage was not private".to_owned());
        }

        let output_name = CString::new(filename)
            .map_err(|_| "packaged Files screenshot name was invalid".to_owned())?;
        if filename.is_empty()
            || filename.as_bytes().contains(&b'/')
            || !filename.starts_with("files-")
            || !filename.ends_with(".png")
        {
            return Err("packaged Files screenshot name was invalid".to_owned());
        }
        let mut output_file = renderer_screenshot_openat(
            root_directory.as_raw_fd(),
            &output_name,
            RENDERER_SCREENSHOT_CREATE_FILE_FLAGS,
            0o600,
        )
        .map_err(|_| "packaged Files screenshot output could not be created".to_owned())?;
        let written = (|| {
            output_file
                .write_all(png)
                .map_err(|_| "packaged Files screenshot output could not be written".to_owned())?;
            output_file
                .sync_all()
                .map_err(|_| "packaged Files screenshot output was not durable".to_owned())?;
            output_file
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| "packaged Files screenshot output was not private".to_owned())?;
            let metadata = output_file
                .metadata()
                .map_err(|_| "packaged Files screenshot output was unreadable".to_owned())?;
            if !metadata.is_file()
                || metadata.permissions().mode() & 0o177 != 0
                || metadata.nlink() != 1
                || metadata.len() != png.len() as u64
            {
                return Err(
                    "packaged Files screenshot output was not an exact private file".to_owned(),
                );
            }
            output_file
                .rewind()
                .map_err(|_| "packaged Files screenshot output was unreadable".to_owned())?;
            let mut stored = Vec::with_capacity(png.len());
            (&mut output_file)
                .take(MAX_RENDERER_SCREENSHOT_BYTES + 1)
                .read_to_end(&mut stored)
                .map_err(|_| "packaged Files screenshot output was unreadable".to_owned())?;
            let stable = output_file
                .metadata()
                .map_err(|_| "packaged Files screenshot output was unreadable".to_owned())?;
            if stored != png
                || stable.dev() != metadata.dev()
                || stable.ino() != metadata.ino()
                || stable.len() != metadata.len()
            {
                return Err("packaged Files screenshot output changed during capture".to_owned());
            }
            Ok(metadata.len())
        })();
        let bytes = match written {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = renderer_screenshot_unlinkat(root_directory.as_raw_fd(), &output_name);
                return Err(error);
            }
        };
        let sha256 = encode_hex(ring::digest::digest(&ring::digest::SHA256, png).as_ref());
        Ok((
            application_data.join("renderer-proof").join(filename),
            width,
            height,
            bytes,
            sha256,
        ))
    }

    #[cfg(target_os = "macos")]
    #[allow(unsafe_code)]
    #[tauri::command(async)]
    async fn renderer_proof_capture_files_screenshot(
        window: tauri::WebviewWindow,
        runtime: State<'_, DesktopRuntime>,
    ) -> Result<String, String> {
        let Some(filename) = runtime.renderer_proof.files_screenshot_name()? else {
            return Ok(Json::object([
                ("schema", Json::text("mesh.renderer-proof-screenshot/v1")),
                ("captured", Json::Bool(false)),
                ("path", Json::Null),
                ("width", Json::Null),
                ("height", Json::Null),
                ("bytes", Json::Null),
                ("sha256", Json::Null),
            ])
            .encode());
        };
        let application_data = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "packaged Files screenshot storage was unavailable".to_owned())?;
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Result<Vec<u8>, String>>(1);
        window
            .with_webview(move |webview| {
                use block2::RcBlock;
                use objc2::{runtime::AnyObject, MainThreadMarker};
                use objc2_app_kit::{
                    NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey, NSImage,
                };
                use objc2_foundation::{NSDictionary, NSError, NSNumber};
                use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

                let Some(marker) = MainThreadMarker::new() else {
                    let _ = sender.send(Err(
                        "packaged Files WebKit snapshot was not on the UI thread".to_owned(),
                    ));
                    return;
                };
                let completion: RcBlock<dyn Fn(*mut NSImage, *mut NSError)> =
                    RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                        let snapshot = if !error.is_null() || image.is_null() {
                            Err("packaged Files WebKit snapshot failed".to_owned())
                        } else {
                            // SAFETY: WebKit owns the callback image for the duration of this
                            // completion block. AppKit returns retained immutable data at each
                            // conversion step, and the bounded PNG is copied before callback return.
                            unsafe { &*image }
                                .TIFFRepresentation()
                                .filter(|data| data.len() <= 64 * 1_024 * 1_024)
                                .and_then(|data| NSBitmapImageRep::imageRepWithData(&data))
                                .and_then(|bitmap| {
                                    let properties = NSDictionary::<
                                        NSBitmapImageRepPropertyKey,
                                        AnyObject,
                                    >::new();
                                    // SAFETY: the empty properties dictionary has the exact key
                                    // and value types AppKit requires for PNG representation.
                                    unsafe {
                                        bitmap.representationUsingType_properties(
                                            NSBitmapImageFileType::PNG,
                                            &properties,
                                        )
                                    }
                                })
                                .filter(|data| data.len() as u64 <= MAX_RENDERER_SCREENSHOT_BYTES)
                                .map(|data| data.to_vec())
                                .ok_or_else(|| {
                                    "packaged Files WebKit snapshot had no pixels".to_owned()
                                })
                        };
                        let _ = sender.send(snapshot);
                    });
                // SAFETY: Tauri supplies the live WKWebView on its owning UI thread. WebKit copies
                // the heap block and invokes it with the snapshot image or an error.
                unsafe {
                    let view: &WKWebView = &*webview.inner().cast();
                    let configuration = WKSnapshotConfiguration::new(marker);
                    let width = NSNumber::new_f64(1_200.0);
                    configuration.setSnapshotWidth(Some(&width));
                    view.takeSnapshotWithConfiguration_completionHandler(
                        Some(&configuration),
                        &completion,
                    );
                }
            })
            .map_err(|_| "packaged Files WebKit snapshot could not start".to_owned())?;
        let png = tauri::async_runtime::spawn_blocking(move || {
            receiver.recv_timeout(Duration::from_secs(12))
        })
        .await
        .map_err(|_| "packaged Files WebKit snapshot stopped unexpectedly".to_owned())?
        .map_err(|_| "packaged Files WebKit snapshot timed out".to_owned())??;
        let screenshot_nonce = filename
            .strip_prefix("files-")
            .and_then(|value| value.strip_suffix(".png"))
            .ok_or_else(|| "packaged Files screenshot name was invalid".to_owned())?
            .to_owned();
        let (path, width, height, bytes, sha256) =
            tauri::async_runtime::spawn_blocking(move || {
                persist_renderer_files_screenshot(&application_data, &filename, &png)
            })
            .await
            .map_err(|_| "packaged Files screenshot storage stopped unexpectedly".to_owned())??;
        eprintln!(
            "mesh-renderer-proof-screenshot:{screenshot_nonce}:{sha256}:{bytes}:{width}:{height}"
        );
        Ok(Json::object([
            ("schema", Json::text("mesh.renderer-proof-screenshot/v1")),
            ("captured", Json::Bool(true)),
            ("path", Json::text(path.display().to_string())),
            ("width", Json::Number(width as u64)),
            ("height", Json::Number(height as u64)),
            ("bytes", Json::Number(bytes)),
            ("sha256", Json::text(sha256)),
        ])
        .encode())
    }

    #[cfg(not(target_os = "macos"))]
    #[tauri::command(async)]
    async fn renderer_proof_capture_files_screenshot(
        runtime: State<'_, DesktopRuntime>,
    ) -> Result<String, String> {
        if runtime.renderer_proof.files_screenshot_name()?.is_some() {
            return Err("packaged Files screenshots require macOS".to_owned());
        }
        Ok(Json::object([
            ("schema", Json::text("mesh.renderer-proof-screenshot/v1")),
            ("captured", Json::Bool(false)),
            ("path", Json::Null),
            ("width", Json::Null),
            ("height", Json::Null),
            ("bytes", Json::Null),
            ("sha256", Json::Null),
        ])
        .encode())
    }

    #[tauri::command]
    fn renderer_proof_report(
        runtime: State<'_, DesktopRuntime>,
        report: String,
    ) -> Result<(), String> {
        let accepted = runtime.renderer_proof.accept(&report)?;
        eprintln!("mesh-renderer-proof:{accepted}");
        Ok(())
    }

    #[tauri::command]
    fn renderer_proof_accept_private_export_confirmation(
        runtime: State<'_, DesktopRuntime>,
        destination: String,
    ) -> Result<bool, String> {
        runtime
            .renderer_proof
            .accept_private_export_confirmation(&destination)
    }

    #[tauri::command]
    fn renderer_proof_failure(
        runtime: State<'_, DesktopRuntime>,
        code: String,
    ) -> Result<&'static str, String> {
        runtime.renderer_proof.report_failure(&code)
    }

    #[tauri::command]
    fn renderer_proof_checkpoint(
        runtime: State<'_, DesktopRuntime>,
        code: String,
    ) -> Result<&'static str, String> {
        runtime.renderer_proof.report_checkpoint(&code)
    }

    #[tauri::command]
    fn renderer_proof_agent_handoff_rescanned(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_installation: String,
        expected_agent_handoff_generation: String,
    ) -> Result<(), String> {
        runtime.renderer_proof.record_agent_handoff_rescan(
            &expected_workspace_root,
            &expected_workspace_installation,
            &expected_agent_handoff_generation,
        )
    }

    #[tauri::command(async)]
    fn open_current_review(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let opened_by = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local review identity is unavailable".to_owned())?
                    .public_key()
                    .public_key();
                runtime
                    .daemon
                    .open_current_review_for_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        opened_by,
                    )
                    .map(|workspace| workspace.to_json().encode())
                    .map_err(|error| daemon_refusal(&error.code, &error.message))
            },
        )
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    async fn render_review_artifact(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        bundle: String,
        target: String,
        object_id: String,
        side: String,
        page_number: Option<usize>,
    ) -> Result<String, String> {
        let artifact = runtime
            .daemon
            .review_artifact_for_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                &bundle,
                &target,
                &object_id,
                &side,
            )
            .map_err(|error| daemon_refusal(&error.code, &error.message))?;
        let version = artifact.version().to_string();
        let digest = artifact.digest().to_string();
        let rendered = tauri::async_runtime::spawn_blocking(move || {
            render_artifact(artifact.path(), artifact.bytes(), page_number)
        })
        .await
        .map_err(|_| "The local artifact preview stopped unexpectedly".to_owned())?
        .map_err(|error| error.to_string())?;
        Ok(rendered_artifact_json(&rendered, &side, &version, &digest).encode())
    }

    fn rendered_artifact_json(
        rendered: &crate::artifact_preview::ArtifactPreview,
        side: &str,
        version: &str,
        digest: &str,
    ) -> Json {
        let (text_source, text_lines, text_sections, text_truncated) =
            rendered.text.as_ref().map_or(
                (Json::Null, Json::Null, Json::Null, Json::Bool(false)),
                |text| {
                    (
                        Json::text(text.source.label()),
                        Json::Array(text.lines.iter().map(Json::text).collect()),
                        Json::Array(
                            text.sections
                                .iter()
                                .map(|section| {
                                    Json::object([
                                        ("label", Json::text(&section.label)),
                                        ("line_start", Json::Number(section.line_start as u64)),
                                        ("line_count", Json::Number(section.line_count as u64)),
                                    ])
                                })
                                .collect(),
                        ),
                        Json::Bool(text.truncated),
                    )
                },
            );
        Json::object([
            ("renderer", Json::text(rendered.renderer.label())),
            ("scope", Json::text(rendered.renderer.scope())),
            ("kind", Json::text(rendered.kind.label())),
            ("side", Json::text(side)),
            ("version_id", Json::text(version)),
            ("content_digest", Json::text(digest)),
            (
                "image_data_url",
                Json::text(format!(
                    "data:image/png;base64,{}",
                    encode_base64(&rendered.png)
                )),
            ),
            ("text_source", text_source),
            ("text_lines", text_lines),
            ("text_sections", text_sections),
            ("text_truncated", text_truncated),
            (
                "page_number",
                rendered
                    .page_number
                    .map_or(Json::Null, |page| Json::Number(page as u64)),
            ),
            (
                "page_count",
                rendered
                    .page_count
                    .map_or(Json::Null, |count| Json::Number(count as u64)),
            ),
            ("rendering_authorizes_approval", Json::Bool(false)),
        ])
    }

    fn review_inspection_family(extension: &str) -> Option<&'static str> {
        match extension {
            "pdf" => Some("pdf"),
            "pptx" => Some("presentation"),
            "docx" => Some("document"),
            "xlsx" => Some("spreadsheet"),
            "png" | "jpg" | "gif" | "webp" => Some("image"),
            _ => None,
        }
    }

    fn review_inspection_families_match<'a>(extensions: impl IntoIterator<Item = &'a str>) -> bool {
        let mut extensions = extensions.into_iter();
        let Some(family) = extensions.next().and_then(review_inspection_family) else {
            return false;
        };
        extensions.all(|extension| review_inspection_family(extension) == Some(family))
    }

    fn review_inspection_destination(
        runtime: &DesktopRuntime,
        value: &str,
        expected_workspace_root: &str,
    ) -> Result<PathBuf, String> {
        let supplied = Path::new(value);
        if !supplied.is_absolute() {
            return Err("Choose an absolute folder for the exact review copies.".to_owned());
        }
        let metadata = fs::symlink_metadata(supplied)
            .map_err(|error| format!("Mesh could not inspect the selected folder: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(
                "Choose a real folder, not a link or file, for the exact review copies.".to_owned(),
            );
        }
        let destination = fs::canonicalize(supplied)
            .map_err(|error| format!("Mesh could not verify the selected folder: {error}"))?;
        let mut protected = vec![
            PathBuf::from(expected_workspace_root),
            runtime.recent.directory().to_path_buf(),
        ];
        let entries = runtime.recent.load_entries().map_err(|error| {
            format!(
                "Mesh could not verify its remembered project boundaries before exporting exact copies: {error}"
            )
        })?;
        for entry in entries {
            protected.push(entry.path().to_path_buf());
            if let Some(root) = entry.export_root() {
                protected.push(root.to_path_buf());
            }
            if let Some(root) = entry.project_root() {
                protected.push(root.to_path_buf());
            }
        }
        if protected.into_iter().any(|root| {
            let canonical = fs::canonicalize(&root).unwrap_or(root);
            destination == canonical || destination.starts_with(&canonical)
        }) {
            return Err(
                "Choose a folder outside the working project and Mesh's private data. Exact review copies never belong inside either workspace."
                    .to_owned(),
            );
        }
        Ok(destination)
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    async fn export_review_artifact_inspection(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        bundle: String,
        target: String,
        object_id: String,
        sides: Vec<String>,
        destination: String,
    ) -> Result<String, String> {
        if sides.is_empty() || sides.len() > 2 {
            return Err("Choose one or two saved sides to inspect.".to_owned());
        }
        let mut selected = std::collections::HashSet::new();
        let sides = sides
            .into_iter()
            .map(|side| match side.as_str() {
                "before" if selected.insert("before") => Ok("before"),
                "after" if selected.insert("after") => Ok("after"),
                _ => Err("The requested artifact sides were invalid or repeated.".to_owned()),
            })
            .collect::<Result<Vec<&'static str>, String>>()?;
        let destination =
            review_inspection_destination(&runtime, &destination, &expected_workspace_root)?;
        let mut artifacts = Vec::with_capacity(sides.len());
        for side in sides {
            let artifact = runtime
                .daemon
                .review_artifact_for_workspace(
                    &expected_workspace_root,
                    &expected_workspace_digest,
                    &expected_workspace_installation,
                    &bundle,
                    &target,
                    &object_id,
                    side,
                )
                .map_err(|error| daemon_refusal(&error.code, &error.message))?;
            let extension =
                inspection_extension(artifact.path(), artifact.bytes()).ok_or_else(|| {
                    "The saved bytes did not match a supported document or image.".to_owned()
                })?;
            let version = artifact.version().to_string();
            let digest = artifact.digest().to_string();
            artifacts.push((side, artifact, extension, version, digest));
        }
        if !review_inspection_families_match(
            artifacts.iter().map(|(_, _, extension, _, _)| *extension),
        ) {
            return Err("The two saved sides were not the same artifact family.".to_owned());
        }
        let export = tauri::async_runtime::spawn_blocking(move || {
            let copies = artifacts
                .iter()
                .map(
                    |(side, artifact, extension, version, digest)| ReviewInspectionCopy {
                        side,
                        reviewed_path: artifact.path(),
                        extension,
                        version,
                        digest,
                        bytes: artifact.bytes(),
                    },
                )
                .collect::<Vec<_>>();
            export_review_inspection(&destination, &bundle, &target, &object_id, &copies)
        })
        .await
        .map_err(|_| "The exact-copy export stopped unexpectedly.".to_owned())?
        .map_err(|error| error.to_string())?;
        let finder = open_native_folder(&export.directory);
        Ok(Json::object([
            ("schema", Json::text("mesh-review-inspection-export/v1")),
            (
                "directory",
                Json::text(export.directory.display().to_string()),
            ),
            (
                "files",
                Json::Array(
                    export
                        .files
                        .into_iter()
                        .map(|file| {
                            Json::object([
                                ("side", Json::text(file.side)),
                                ("path", Json::text(file.path.display().to_string())),
                                ("version_id", Json::text(file.version)),
                                ("content_digest", Json::text(file.digest)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("working_folder_unchanged", Json::Bool(true)),
            ("document_content_opened", Json::Bool(false)),
            ("finder_opened", Json::Bool(finder.is_ok())),
            ("warning", finder.err().map_or(Json::Null, Json::text)),
        ])
        .encode())
    }

    /// Materialize one immutable saved side into Mesh-owned private storage, then perform only the
    /// exact native action named by the renderer. The response deliberately carries no path: the
    /// renderer can request a closed action but never gains ambient filesystem authority.
    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    async fn open_review_artifact_inspection(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        bundle: String,
        target: String,
        object_id: String,
        side: String,
        expected_version_id: String,
        expected_content_digest: String,
        action: String,
    ) -> Result<String, String> {
        let side = match side.as_str() {
            "before" => "before",
            "after" => "after",
            _ => return Err("The requested saved side was not recognized.".to_owned()),
        };
        if !matches!(
            action.as_str(),
            "open-entry" | "reveal-entry" | "open-folder"
        ) {
            return Err("The saved-side action was not recognized.".to_owned());
        }
        let artifact = runtime
            .daemon
            .review_artifact_for_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                &bundle,
                &target,
                &object_id,
                side,
            )
            .map_err(|error| daemon_refusal(&error.code, &error.message))?;
        let version = artifact.version().to_string();
        let digest = artifact.digest().to_string();
        let extension = admitted_review_inspection_extension(
            artifact.path(),
            artifact.bytes(),
            &version,
            &digest,
            &expected_version_id,
            &expected_content_digest,
            &action,
        )?;
        let inspection_root = runtime.recent.directory().join("review-inspection");
        if let Err(error) = fs::create_dir(&inspection_root) {
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(format!(
                    "Mesh could not create its private review-copy folder: {error}"
                ));
            }
        }
        let root_metadata = fs::symlink_metadata(&inspection_root).map_err(|error| {
            format!("Mesh could not inspect its private review-copy folder: {error}")
        })?;
        if !root_metadata.file_type().is_dir() || root_metadata.file_type().is_symlink() {
            return Err("Mesh's private review-copy folder is not a real directory.".to_owned());
        }
        let mut root_permissions = root_metadata.permissions();
        root_permissions.set_mode(0o700);
        fs::set_permissions(&inspection_root, root_permissions).map_err(|error| {
            format!("Mesh could not protect its private review-copy folder: {error}")
        })?;
        prune_app_owned_review_inspections(
            &inspection_root,
            MAX_RETAINED_REVIEW_INSPECTIONS.saturating_sub(1),
        )
        .map_err(|error| error.to_string())?;
        let export = tauri::async_runtime::spawn_blocking({
            let inspection_root = inspection_root.clone();
            let bundle = bundle.clone();
            let target = target.clone();
            let object_id = object_id.clone();
            move || {
                export_review_inspection(
                    &inspection_root,
                    &bundle,
                    &target,
                    &object_id,
                    &[ReviewInspectionCopy {
                        side,
                        reviewed_path: artifact.path(),
                        extension,
                        version: &version,
                        digest: &digest,
                        bytes: artifact.bytes(),
                    }],
                )
                .map(|export| (export, version, digest))
            }
        })
        .await
        .map_err(|_| "The exact review-copy preparation stopped unexpectedly.".to_owned())?
        .map_err(|error| error.to_string())?;
        let (export, version, digest) = export;
        let file = export
            .files
            .first()
            .ok_or_else(|| "The exact saved-side copy was not created.".to_owned())?;
        let (reference, launcher_action) = if action == "open-folder" {
            (
                stable_native_reference(&export.directory, true)?,
                "open-entry",
            )
        } else {
            (stable_native_reference(&file.path, false)?, action.as_str())
        };
        #[cfg(target_os = "macos")]
        let default_application = if action == "open-entry" {
            Some(default_native_application(&file.path)?)
        } else if action == "open-folder" {
            Some(PathBuf::from(FINDER_APPLICATION_PATH))
        } else {
            None
        };
        #[cfg(not(target_os = "macos"))]
        let default_application: Option<PathBuf> = None;
        open_native_entry_with(
            Path::new(NATIVE_FOLDER_OPENER),
            &reference,
            launcher_action,
            default_application.as_deref(),
            NATIVE_FOLDER_OPEN_TIMEOUT,
        )?;
        Ok(Json::object([
            ("schema", Json::text("mesh.review-side-open/v1")),
            ("side", Json::text(side)),
            ("action", Json::text(action)),
            ("version_id", Json::text(version)),
            ("content_digest", Json::text(digest)),
            ("opened", Json::Bool(true)),
            ("working_folder_unchanged", Json::Bool(true)),
        ])
        .encode())
    }

    fn approval_status_json(
        credential: Option<&SecureEnclaveApprovalCredential>,
        unavailable_reason: Option<&str>,
    ) -> String {
        Json::object([
            ("enrolled", Json::Bool(credential.is_some())),
            ("available", Json::Bool(unavailable_reason.is_none())),
            (
                "unavailable_reason",
                unavailable_reason.map_or(Json::Null, Json::text),
            ),
            (
                "credential_id",
                credential.map_or(Json::Null, |credential| {
                    Json::text(credential.credential().id().to_string())
                }),
            ),
            ("algorithm", Json::text(mesh_approval::APPROVAL_ALGORITHM)),
            (
                "user_verification",
                Json::text(mesh_approval::APPROVAL_USER_VERIFICATION),
            ),
            (
                "application_scope",
                Json::text(mesh_approval::APPROVAL_APPLICATION_SCOPE),
            ),
        ])
        .encode()
    }

    fn native_approval_prompt(
        workspace_root: &str,
        preview: &mesh_daemon::HumanApprovalPreview,
        draft: &HumanApprovalReceiptDraft,
        git_target: Option<&std::path::Path>,
    ) -> String {
        let expected = draft.expected();
        let context = expected.context();
        let git_effect = git_target.map_or_else(
            || "".to_owned(),
            |target| {
                format!(
                    "\n\nGIT EXPORT\nAfter approval, Mesh will create a new review branch in {}. It will not switch the current branch or change the original working files.",
                    target.display()
                )
            },
        );
        format!(
            "Approve all changes in this exact recorded version?\n\nLocal folder (orientation only): {workspace_root}\n\nSIGNED APPROVAL STATEMENT\nWorkspace identity: {}\nExpected shared head: {}\nReviewed head: {}\nReview bundle: {}\nSelected changes: {}\nConflict resolutions: {}\nValidation evidence: {}\nPolicy epoch: {}\nApproving credential: {}\nAlgorithm: {APPROVAL_ALGORITHM}\nCredential public key: {}\nApplication: {APPROVAL_APPLICATION_SCOPE}\nUser verification: {APPROVAL_USER_VERIFICATION}\nCeremony challenge: {}\nDecision: approve\nCanonical statement BLAKE3: {}\n\nREVIEW PRESENTATION (derived from that bundle)\nPresentation BLAKE3: {}\n{}{git_effect}\nThis advances the protected shared version. Cancel leaves the review and private work unchanged.",
            encode_hex(context.workspace_id().as_bytes()),
            context.expected_canonical_head(),
            context.reviewed_actor_head(),
            context.review_bundle(),
            context.selected_changes(),
            context.conflict_resolutions(),
            context.validation_digest(),
            context.policy_epoch().value(),
            expected.credential().id(),
            encode_hex(expected.credential().public_key()),
            encode_hex(expected.challenge()),
            draft.statement_digest(),
            preview.presentation_digest(),
            preview.change_summary(),
        )
    }

    const MAX_NATIVE_APPROVAL_PROMPT_BYTES: usize = 48 * 1024;
    fn attachment_review_trust() -> mesh_daemon::TrustedReviewers {
        SecureEnclaveApprovalCredential::load().map_or_else(
            |_| mesh_daemon::TrustedReviewers::default(),
            |credential| {
                mesh_daemon::TrustedReviewers::with_human_credentials([credential
                    .credential()
                    .clone()])
            },
        )
    }

    #[tauri::command(async)]
    fn attachment_approval_status(
        host: State<'_, Arc<AttachmentHost>>,
        runtime: State<'_, DesktopRuntime>,
        id: String,
    ) -> Result<String, String> {
        let credential = Json::parse(&approval_credential_status(runtime)?)
            .map_err(|_| "Approval availability could not be read".to_owned())?;
        let history = host.review_history(&id)?;
        let main = history.accepted_main(&attachment_review_trust());
        Ok(Json::object([
            ("schema", Json::text("mesh.desktop-attachment-main/v1")),
            ("project", Json::text(id)),
            ("credential", credential),
            ("main_available", Json::Bool(main.is_ok())),
            ("main", main.unwrap_or(Json::Null)),
        ])
        .encode())
    }

    #[tauri::command]
    async fn preview_attached_main_integration(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        bundle: String,
        target: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            let history = host.review_history(&id)?;
            let preview = history.preview_main_integration(
                &bundle, &target, &attachment_review_trust(),
                mesh_daemon::project_attachment::ObservationLimits::default(),
            ).map_err(|_| "Working-folder comparison is unavailable. Refresh Mesh main and retry after any ongoing file changes settle.".to_owned())?;
            Ok(Json::object([
                ("schema", Json::text("mesh.desktop-attachment-integration/v1")),
                ("project", Json::text(id)), ("preview", preview),
            ]).encode())
        }).await.map_err(|_| "Working-folder comparison stopped".to_owned())?
    }

    #[tauri::command(async)]
    fn open_attached_folder(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
    ) -> Result<(), String> {
        let history = host.review_history(&id)?;
        let reference = history
            .project()
            .native_folder_reference()
            .map_err(|_| "The original project folder is unavailable or its identity changed")?;
        open_native_folder(&reference)
    }

    #[tauri::command]
    async fn open_attached_version_lane(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        version: String,
        request: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.open_version_lane(&id, &version, &request)
        })
        .await
        .map_err(|_| "Lane allocation stopped; retry the same request".to_owned())?
    }

    #[tauri::command]
    async fn inspect_attached_recovery(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        transaction: Option<String>,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.file_recovery(&id, transaction.as_deref(), &attachment_review_trust())
        })
        .await
        .map_err(|_| "Retained-file inspection stopped".to_owned())?
    }

    #[tauri::command(async)]
    fn restore_attached_retained_file(
        app: tauri::AppHandle,
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        transaction: String,
    ) -> Result<String, String> {
        restore_retained_file(app, host, id, transaction, None)
    }

    #[tauri::command(async)]
    fn restore_attached_group_file(
        app: tauri::AppHandle,
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        group: String,
        transaction: String,
    ) -> Result<String, String> {
        restore_retained_file(app, host, id, transaction, Some(group))
    }

    #[tauri::command(async)]
    fn restore_attached_retained_entry(
        app: tauri::AppHandle,
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        transaction: String,
        group: Option<String>,
    ) -> Result<String, String> {
        let (history, generation) = host.file_change_history(&id)?;
        let trust = attachment_review_trust();
        let mut root = history
            .file_recovery_root(false)
            .map_err(|_| "Entry recovery storage is unavailable")?
            .ok_or("There are no retained entries for this project")?;
        if let Some(group) = &group {
            history
                .inspect_main_integration_group(
                    &root,
                    group,
                    &trust,
                    mesh_daemon::project_attachment::ObservationLimits::default(),
                )
                .map_err(|_| "The recovery group could not be verified")?;
            root = root.join(group);
        }
        let prepared = history.prepare_retained_entry_restoration(&root, &transaction, &trust,
            mesh_daemon::project_attachment::ObservationLimits::default())
            .map_err(|_| "This retained entry cannot currently be restored. Refresh recovery and inspect changed or unavailable work.")?;
        let prompt = crate::attachment_recovery::entry_restoration_confirmation(
            &id,
            history.project().root(),
            &prepared,
        )?;
        let next = crate::attachment_recovery::transaction_id(prepared.recovery_path())?;
        if !app
            .dialog()
            .message(prompt)
            .title("Restore retained file or folder")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Restore this entry".into(),
                "Cancel".into(),
            ))
            .blocking_show()
        {
            return Err(
                "Restoration was cancelled; the prepared recovery record is retained".into(),
            );
        }
        let outcome = host.confirm_file_change(&id, generation, || {
            prepared.apply(&attachment_review_trust()).map_err(|_| {
                "Restoration was not confirmed. Inspect recovery before retrying; working entries may have changed.".to_owned()
            })
        })?;
        Ok(Json::object([
            (
                "schema",
                Json::text("mesh.desktop-attachment-entry-change/v1"),
            ),
            ("project", Json::text(id)),
            ("transaction", Json::text(next)),
            ("group", group.map_or(Json::Null, Json::text)),
            ("outcome", outcome),
        ])
        .encode())
    }

    fn restore_retained_file(
        app: tauri::AppHandle,
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        transaction: String,
        group: Option<String>,
    ) -> Result<String, String> {
        let (history, generation) = host.file_change_history(&id)?;
        let trust = attachment_review_trust();
        let mut root = history
            .file_recovery_root(false)
            .map_err(|_| "File recovery storage is unavailable")?
            .ok_or("There are no retained files for this project")?;
        if let Some(group) = &group {
            history
                .inspect_main_integration_group(
                    &root,
                    group,
                    &trust,
                    mesh_daemon::project_attachment::ObservationLimits::default(),
                )
                .map_err(|_| "The recovery group could not be verified")?;
            root = root.join(group);
        }
        let prepared = history.prepare_retained_restoration(
            &root, &transaction, &trust,
            mesh_daemon::project_attachment::ObservationLimits::default(),
        ).map_err(|_| "This retained file cannot currently be restored. Refresh recovery and check for changed or unavailable files.")?;
        let prompt = crate::attachment_recovery::confirmation(
            "Restore retained work",
            &id,
            history.project().root(),
            prepared.proposal(),
            prepared.current_content(),
            prepared.restored_content(),
        )?;
        let next = crate::attachment_recovery::transaction_id(prepared.recovery_path())?;
        if !app
            .dialog()
            .message(prompt)
            .title("Restore retained work")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Restore this file".into(),
                "Cancel".into(),
            ))
            .blocking_show()
        {
            return Err(
                "Restoration was cancelled; the prepared recovery record is retained".into(),
            );
        }
        let outcome = host.confirm_file_change(&id, generation, || prepared.apply(&attachment_review_trust())
            .map_err(|_| "Restoration was not confirmed. Inspect recovery before retrying; working files may have changed.".to_owned()))?;
        let result = crate::attachment_recovery::result(&id, &next, outcome);
        if let Some(group) = group {
            let mut value = mesh_daemon::ipc::Json::parse(&result)
                .map_err(|_| "Native restoration result unavailable")?;
            if let mesh_daemon::ipc::Json::Object(fields) = &mut value {
                fields.push(("group".into(), mesh_daemon::ipc::Json::text(group)));
            }
            Ok(value.encode())
        } else {
            Ok(result)
        }
    }

    #[tauri::command]
    async fn inspect_attached_group_recovery(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        group: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.group_recovery(&id, &group, &attachment_review_trust())
        })
        .await
        .map_err(|_| "Group recovery inspection stopped".to_owned())?
    }

    #[tauri::command]
    async fn inspect_attached_group_file(
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        group: String,
        transaction: String,
    ) -> Result<String, String> {
        let host = Arc::clone(host.inner());
        tauri::async_runtime::spawn_blocking(move || {
            host.group_file_recovery(&id, &group, &transaction, &attachment_review_trust())
        })
        .await
        .map_err(|_| "Group file inspection stopped".to_owned())?
    }

    #[tauri::command(async)]
    fn apply_attached_main_group(
        app: tauri::AppHandle,
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        bundle: String,
        target: String,
    ) -> Result<String, String> {
        use mesh_daemon::ipc::Json;
        let (history, generation) = host.file_change_history(&id)?;
        let root = history
            .file_recovery_root(true)
            .map_err(|_| "Private recovery storage is unavailable")?
            .ok_or("Private recovery storage is unavailable")?;
        let prepared = history.prepare_main_integration(&bundle, &target, &root, &attachment_review_trust(), mesh_daemon::project_attachment::ObservationLimits::default())
            .map_err(|_| "This complete review cannot currently be applied. Refresh the comparison and resolve divergent files. Directory changes require further support.")?;
        let group = crate::attachment_recovery::transaction_id(prepared.recovery_path())?;
        let prompt = crate::attachment_recovery::group_confirmation(
            &id,
            history.project().root(),
            &prepared,
        )?;
        if !app
            .dialog()
            .message(prompt)
            .title("Apply accepted changes")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Apply these changes".into(),
                "Cancel".into(),
            ))
            .blocking_show()
        {
            return Err("Application was cancelled; the prepared group remains in recovery".into());
        }
        let outcome = host.confirm_file_change(&id, generation, || prepared.apply(&attachment_review_trust())
            .map_err(|_| "The group outcome is uncertain. Inspect recovery before retrying; some working files may have changed.".to_owned()))?;
        Ok(Json::object([
            (
                "schema",
                Json::text("mesh.desktop-attachment-group-change/v1"),
            ),
            ("project", Json::text(id)),
            ("group", Json::text(group)),
            ("outcome", outcome),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn apply_attached_main_file(
        app: tauri::AppHandle,
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        bundle: String,
        target: String,
        path: String,
    ) -> Result<String, String> {
        let (history, generation) = host.file_change_history(&id)?;
        let root = history
            .file_recovery_root(true)
            .map_err(|_| "Private file recovery storage is unavailable")?
            .ok_or("Private file recovery storage is unavailable")?;
        let prepared = history.prepare_main_file_integration(
            &bundle, &target, &path, &root, &attachment_review_trust(),
            mesh_daemon::project_attachment::ObservationLimits::default(),
        ).map_err(|_| "This file cannot currently be applied. It must still match the approved base, with recovery storage on the same volume. Refresh main and the working-folder comparison.")?;
        let prompt = crate::attachment_recovery::confirmation(
            "Apply one file from Mesh main",
            &id,
            history.project().root(),
            prepared.proposal(),
            prepared.current_content(),
            prepared.proposed_content(),
        )?;
        let transaction = crate::attachment_recovery::transaction_id(prepared.recovery_path())?;
        if !app
            .dialog()
            .message(prompt)
            .title("Apply one file from Mesh main")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Apply this file".into(),
                "Cancel".into(),
            ))
            .blocking_show()
        {
            return Err(
                "Application was cancelled; the prepared recovery record is retained".into(),
            );
        }
        let outcome = host.confirm_file_change(&id, generation, || prepared.apply(&attachment_review_trust())
            .map_err(|_| "Application was not confirmed. Inspect recovery before retrying; working files may have changed.".to_owned()))?;
        Ok(crate::attachment_recovery::result(
            &id,
            &transaction,
            outcome,
        ))
    }

    #[tauri::command(async)]
    fn approve_attached_review(
        app: tauri::AppHandle,
        host: State<'_, Arc<AttachmentHost>>,
        id: String,
        bundle: String,
        target: String,
    ) -> Result<String, String> {
        let credential =
            SecureEnclaveApprovalCredential::load().map_err(|error| error.to_string())?;
        let trust = mesh_daemon::TrustedReviewers::with_human_credentials([credential
            .credential()
            .clone()]);
        // Retain the exact native attachment through the entire ceremony. Never resolve a fresh
        // source/store handle from a renderer path after the person confirms.
        let history = host.review_history(&id)?;
        let preview = history.approval_preview(&bundle, &target, &trust)
            .map_err(|_| "This review cannot advance the current Mesh main. Refresh main and request a new review if its base changed.".to_owned())?;
        let expected = ExpectedHumanApproval::new(
            preview.context().clone(),
            credential.credential().clone(),
            fresh_approval_challenge().map_err(|error| error.to_string())?,
        );
        let draft = HumanApprovalReceiptDraft::new(expected, ApprovalDecision::Approve);
        let prompt = format!("This approves a saved version as Mesh main. Your working files and Git remain unchanged.\n\n{}",
            native_approval_prompt(&format!("{:?}", history.project().root()), &preview, &draft, None));
        if prompt.len() > MAX_NATIVE_APPROVAL_PROMPT_BYTES {
            return Err("This review is too large to show completely in the native approval dialog. Mesh will not omit approval details.".to_owned());
        }
        if !app
            .dialog()
            .message(prompt)
            .title("Approve to Mesh main")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Approve this version".to_owned(),
                "Cancel".to_owned(),
            ))
            .blocking_show()
        {
            return Err("Approval was cancelled".to_owned());
        }
        let receipt = credential
            .approve(draft)
            .map_err(|error| error.to_string())?;
        let head = history
            .approve_review(&bundle, &target, &receipt.canonical_bytes(), &trust)
            .map_err(|_| {
                "Approval could not be confirmed. Refresh Mesh main before retrying.".to_owned()
            })?;
        Ok(Json::object([
            ("schema", Json::text("mesh.desktop-attachment-approval/v1")),
            ("project", Json::text(id)),
            ("bundle", Json::text(bundle)),
            ("target", Json::text(target)),
            ("head", Json::text(head.to_string())),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn approval_credential_status(runtime: State<'_, DesktopRuntime>) -> Result<String, String> {
        if let Err(error) = SecureEnclaveApprovalCredential::availability() {
            return Ok(approval_status_json(None, Some(&error.to_string())));
        }
        match SecureEnclaveApprovalCredential::load() {
            Ok(credential) => {
                runtime
                    .daemon
                    .trust_human_approval_credential(credential.credential().clone());
                Ok(approval_status_json(Some(&credential), None))
            }
            Err(SecureEnclaveApprovalError::NotEnrolled) => Ok(approval_status_json(None, None)),
            // Availability is durable presentation state, not a failed user action. Return the
            // platform refusal so an ad-hoc or otherwise unentitled build cannot present a setup
            // button that is guaranteed to fail after the person has reviewed their work.
            // Enrolment and signing themselves remain fail-closed below.
            Err(error) => Ok(approval_status_json(None, Some(&error.to_string()))),
        }
    }

    #[tauri::command(async)]
    fn enroll_approval_credential(
        app: tauri::AppHandle,
        runtime: State<'_, DesktopRuntime>,
    ) -> Result<String, String> {
        let confirmed = app
            .dialog()
            .message(
                "Set up a device-only Mesh approval credential on this Mac?\n\nThe private key cannot leave the Secure Enclave. Every approval will still require Touch ID or your Mac password. This does not approve any workspace version.",
            )
            .title("Set up Mesh approvals")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Set up approvals".to_owned(),
                "Not now".to_owned(),
            ))
            .blocking_show();
        if !confirmed {
            return Err("Approval setup was cancelled".to_owned());
        }
        let credential =
            SecureEnclaveApprovalCredential::enroll().map_err(|error| error.to_string())?;
        runtime
            .daemon
            .trust_human_approval_credential(credential.credential().clone());
        Ok(approval_status_json(Some(&credential), None))
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    fn approve_current_review(
        app: tauri::AppHandle,
        runtime: State<'_, DesktopRuntime>,
        bundle: String,
        target: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        export_to_git: bool,
    ) -> Result<String, String> {
        let credential =
            SecureEnclaveApprovalCredential::load().map_err(|error| error.to_string())?;
        runtime
            .daemon
            .trust_human_approval_credential(credential.credential().clone());
        let preview = runtime
            .daemon
            .human_approval_preview_for_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                &bundle,
                &target,
            )
            .map_err(|error| daemon_refusal(&error.code, &error.message))?;
        let expected = ExpectedHumanApproval::new(
            preview.context().clone(),
            credential.credential().clone(),
            fresh_approval_challenge().map_err(|error| error.to_string())?,
        );
        #[cfg(feature = "git-integration")]
        let git_target = if export_to_git {
            Some(approved_git_target(
                &runtime.recent,
                std::path::Path::new(&expected_workspace_root),
            )?)
        } else {
            None
        };
        #[cfg(not(feature = "git-integration"))]
        let git_target: Option<(std::path::PathBuf, ())> = if export_to_git {
            return Err("This Mesh build does not include Git export support".to_owned());
        } else {
            None
        };
        let draft = HumanApprovalReceiptDraft::new(expected, ApprovalDecision::Approve);
        let prompt = native_approval_prompt(
            &expected_workspace_root,
            &preview,
            &draft,
            git_target.as_ref().map(|(path, _)| path.as_path()),
        );
        if prompt.len() > MAX_NATIVE_APPROVAL_PROMPT_BYTES {
            return Err(
                "This review is too large to show completely in the native approval dialog. Split it into a smaller saved version before approving; Mesh will not omit approval details."
                    .to_owned(),
            );
        }
        let confirmed = app
            .dialog()
            .message(prompt)
            .title("Approve to shared version")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Approve this version".to_owned(),
                "Cancel".to_owned(),
            ))
            .blocking_show();
        if !confirmed {
            return Err("Approval was cancelled; no shared version changed".to_owned());
        }
        let receipt = credential
            .approve(draft)
            .map_err(|error| error.to_string())?;
        let receipt_bytes = receipt.canonical_bytes();
        let receipt_digest = ApprovalBlake3::digest_bytes(&receipt_bytes);
        let receipt_hex = encode_hex(&receipt_bytes);
        #[cfg(feature = "git-integration")]
        let actors = [preview.review_bundle().author()];
        #[cfg(feature = "git-integration")]
        let git_preview = if let Some((target_root, target_anchor)) = git_target {
            let source = mesh_git_bridge::ApprovedGitExportSource::new(
                &receipt_bytes,
                receipt.draft().expected(),
                preview.review_bundle(),
                preview.approved_state(),
                &actors,
            );
            let export = mesh_git_bridge::preview_approved_git_export(
                std::path::Path::new(&expected_workspace_root),
                &target_root,
                &target_anchor,
                &source,
            )
            .map_err(|error| {
                format!(
                    "Git export was refused before approval; no shared version changed: {error}"
                )
            })?;
            Some((target_root, target_anchor, export))
        } else {
            None
        };
        // Native approval confirmation can remain open while another Mesh process acquires
        // custody of this managed folder. Re-enter the shared recent-workspace transaction only
        // for the semantic mutation so a stale confirmation cannot advance shared history after
        // that handoff. Secure Enclave interaction and the native dialog stay outside the lock.
        let workspace = with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                runtime
                    .daemon
                    .approve_review_for_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        &bundle,
                        &target,
                        &receipt_hex,
                    )
                    .map_err(|error| daemon_refusal(&error.code, &error.message))
            },
        )?;
        #[cfg(feature = "git-integration")]
        let git_export = if let Some((target_root, target_anchor, git_preview)) = git_preview {
            let source = mesh_git_bridge::ApprovedGitExportSource::new(
                &receipt_bytes,
                receipt.draft().expected(),
                preview.review_bundle(),
                preview.approved_state(),
                &actors,
            );
            match confirm_shared_review_git_export(
                &runtime.daemon,
                &workspace.root,
                &workspace.digest,
                &workspace.installation,
                &git_preview,
                &target_anchor,
                &source,
            ) {
                Ok(exported) => Json::object([
                    ("status", Json::text("exported")),
                    ("target", Json::text(target_root.display().to_string())),
                    ("branch", Json::text(exported.branch())),
                    ("commit", Json::text(exported.commit())),
                    ("approval_ref", Json::text(exported.approval_ref())),
                    ("already_present", Json::Bool(exported.already_present())),
                    ("message", Json::Null),
                ]),
                Err(error) => Json::object([
                    ("status", Json::text("failed")),
                    ("target", Json::text(target_root.display().to_string())),
                    ("branch", Json::Null),
                    ("commit", Json::Null),
                    ("approval_ref", Json::Null),
                    ("already_present", Json::Bool(false)),
                    ("message", Json::text(error.to_string())),
                ]),
            }
        } else {
            Json::object([
                ("status", Json::text("not-requested")),
                ("target", Json::Null),
                ("branch", Json::Null),
                ("commit", Json::Null),
                ("approval_ref", Json::Null),
                ("already_present", Json::Bool(false)),
                ("message", Json::Null),
            ])
        };
        #[cfg(not(feature = "git-integration"))]
        let git_export = Json::object([
            ("status", Json::text("not-requested")),
            ("target", Json::Null),
            ("branch", Json::Null),
            ("commit", Json::Null),
            ("approval_ref", Json::Null),
            ("already_present", Json::Bool(false)),
            ("message", Json::Null),
        ]);
        let shared_version = preview.context().reviewed_actor_head().to_string();
        let statement_digest = receipt.draft().statement_digest().to_string();
        let credential_id = receipt.draft().expected().credential().id().to_string();
        app.dialog()
            .message(format!(
                "Approval completed and survived verification.\n\nShared version: {shared_version}\nReview bundle: {bundle}\nCredential: {credential_id}\nUser verification: {APPROVAL_USER_VERIFICATION}\nCanonical statement BLAKE3: {statement_digest}\nCanonical receipt BLAKE3: {receipt_digest}\n\n{}",
                match git_export.get("status").and_then(Json::as_text) {
                    Some("exported") => format!(
                        "Git review branch: {}\nOriginal working files were not changed.",
                        git_export.get("branch").and_then(Json::as_text).unwrap_or("unavailable")
                    ),
                    Some("failed") => format!(
                        "The shared approval succeeded, but Git export was refused: {}",
                        git_export.get("message").and_then(Json::as_text).unwrap_or("unknown refusal")
                    ),
                    _ => "No Git export was requested.".to_owned(),
                }
            ))
            .title("Mesh approval receipt")
            .buttons(MessageDialogButtons::Ok)
            .blocking_show();
        Ok(Json::object([
            ("workspace", workspace.to_json()),
            (
                "receipt",
                Json::object([
                    ("protocol", Json::text("mesh.v1.approval-receipt")),
                    (
                        "canonical_receipt_blake3",
                        Json::text(receipt_digest.to_string()),
                    ),
                    ("canonical_statement_blake3", Json::text(statement_digest)),
                    ("credential_id", Json::text(credential_id)),
                    ("review_bundle", Json::text(bundle)),
                    ("shared_version", Json::text(shared_version)),
                    ("user_verification", Json::text(APPROVAL_USER_VERIFICATION)),
                ]),
            ),
            ("git_export", git_export),
        ])
        .encode())
    }

    /// Re-enter exact shared-workspace authority after the native confirmation returns.
    ///
    /// The Git transaction stays inside `custody -> workspace_open`; no Recent lock is acquired.
    /// A second desktop may therefore either acquire agent custody first or wait for the complete
    /// ref publication, but it cannot cross a stale confirmation between preview and execution.
    #[cfg(feature = "git-integration")]
    fn confirm_shared_review_git_export(
        daemon: &LiveDaemon,
        expected_workspace_root: &str,
        expected_workspace_digest: &str,
        expected_workspace_installation: &str,
        preview: &mesh_git_bridge::GitExportPreview,
        target_anchor: &mesh_git_bridge::GitProvenanceAnchor,
        source: &mesh_git_bridge::ApprovedGitExportSource<'_>,
    ) -> Result<mesh_git_bridge::ApprovedGitExport, String> {
        daemon
            .with_verified_shared_managed_workspace(
                expected_workspace_root,
                expected_workspace_digest,
                expected_workspace_installation,
                || {
                    mesh_git_bridge::confirm_approved_git_export(preview, target_anchor, source)
                        .map_err(|error| {
                            ManagedTextFileError::Recovery(format!(
                                "Git export transaction was refused: {error}"
                            ))
                        })
                },
            )
            .map_err(|error| format!("Git export was refused: {error}"))
    }

    #[tauri::command(async)]
    fn export_shared_review_to_git(
        app: tauri::AppHandle,
        runtime: State<'_, DesktopRuntime>,
        bundle: String,
        target: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        #[cfg(not(feature = "git-integration"))]
        {
            let _ = (
                app,
                runtime,
                bundle,
                target,
                expected_workspace_root,
                expected_workspace_digest,
                expected_workspace_installation,
            );
            return Err("This Mesh build does not include Git export support".to_owned());
        }
        #[cfg(feature = "git-integration")]
        {
            let credential =
                SecureEnclaveApprovalCredential::load().map_err(|error| error.to_string())?;
            runtime
                .daemon
                .trust_human_approval_credential(credential.credential().clone());
            let approval = runtime
                .daemon
                .durable_human_approval_for_workspace(
                    &expected_workspace_root,
                    &expected_workspace_digest,
                    &expected_workspace_installation,
                    &bundle,
                    &target,
                )
                .map_err(|error| daemon_refusal(&error.code, &error.message))?;
            let (target_root, target_anchor) = approved_git_target(
                &runtime.recent,
                std::path::Path::new(&expected_workspace_root),
            )?;
            let actors = [approval.preview().review_bundle().author()];
            let source = mesh_git_bridge::ApprovedGitExportSource::new(
                approval.receipt(),
                approval.expected(),
                approval.preview().review_bundle(),
                approval.preview().approved_state(),
                &actors,
            );
            let preview = mesh_git_bridge::preview_approved_git_export(
                std::path::Path::new(&expected_workspace_root),
                &target_root,
                &target_anchor,
                &source,
            )
            .map_err(|error| format!("Git export was refused: {error}"))?;
            let confirmed = app
                .dialog()
                .message(format!(
                    "Create Git review branch {} in {} from the already-approved shared version?\n\nMesh will not switch the current branch or change the original working files.",
                    preview.branch(),
                    target_root.display(),
                ))
                .title("Create Git review branch")
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Create review branch".to_owned(),
                    "Cancel".to_owned(),
                ))
                .blocking_show();
            if !confirmed {
                return Err("Git export was cancelled; no repository ref changed".to_owned());
            }
            let exported = confirm_shared_review_git_export(
                &runtime.daemon,
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                &preview,
                &target_anchor,
                &source,
            )?;
            let workspace = runtime
                .daemon
                .workspace_state()
                .map_err(|error| daemon_refusal(&error.code, &error.message))?;
            Ok(Json::object([
                ("workspace", workspace.to_json()),
                (
                    "git_export",
                    Json::object([
                        ("status", Json::text("exported")),
                        ("target", Json::text(target_root.display().to_string())),
                        ("branch", Json::text(exported.branch())),
                        ("commit", Json::text(exported.commit())),
                        ("approval_ref", Json::text(exported.approval_ref())),
                        ("already_present", Json::Bool(exported.already_present())),
                        ("message", Json::Null),
                    ]),
                ),
            ])
            .encode())
        }
    }

    #[tauri::command(async)]
    fn inspect_shared_review_git_export(
        runtime: State<'_, DesktopRuntime>,
        bundle: String,
        target: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        #[cfg(not(feature = "git-integration"))]
        {
            let _ = (
                runtime,
                bundle,
                target,
                expected_workspace_root,
                expected_workspace_digest,
                expected_workspace_installation,
            );
            return Err("This Mesh build does not include Git export support".to_owned());
        }
        #[cfg(feature = "git-integration")]
        {
            let credential =
                SecureEnclaveApprovalCredential::load().map_err(|error| error.to_string())?;
            runtime
                .daemon
                .trust_human_approval_credential(credential.credential().clone());
            let approval = runtime
                .daemon
                .durable_human_approval_for_workspace(
                    &expected_workspace_root,
                    &expected_workspace_digest,
                    &expected_workspace_installation,
                    &bundle,
                    &target,
                )
                .map_err(|error| daemon_refusal(&error.code, &error.message))?;
            let (target_root, target_anchor) = approved_git_target(
                &runtime.recent,
                std::path::Path::new(&expected_workspace_root),
            )?;
            let actors = [approval.preview().review_bundle().author()];
            let source = mesh_git_bridge::ApprovedGitExportSource::new(
                approval.receipt(),
                approval.expected(),
                approval.preview().review_bundle(),
                approval.preview().approved_state(),
                &actors,
            );
            let preview = mesh_git_bridge::preview_approved_git_export(
                std::path::Path::new(&expected_workspace_root),
                &target_root,
                &target_anchor,
                &source,
            )
            .map_err(|error| format!("Git export inspection was refused: {error}"))?;
            let inspected =
                mesh_git_bridge::inspect_approved_git_export(&preview, &target_anchor, &source)
                    .map_err(|error| format!("Git export inspection was refused: {error}"))?;
            let workspace = runtime
                .daemon
                .workspace_state()
                .map_err(|error| daemon_refusal(&error.code, &error.message))?;
            let git_export = if let Some(exported) = inspected {
                Json::object([
                    ("status", Json::text("exported")),
                    ("target", Json::text(target_root.display().to_string())),
                    ("branch", Json::text(exported.branch())),
                    ("commit", Json::text(exported.commit())),
                    ("approval_ref", Json::text(exported.approval_ref())),
                    ("already_present", Json::Bool(true)),
                    ("message", Json::Null),
                ])
            } else {
                Json::object([
                    ("status", Json::text("missing")),
                    ("target", Json::text(target_root.display().to_string())),
                    ("branch", Json::text(preview.branch())),
                    ("commit", Json::Null),
                    ("approval_ref", Json::Null),
                    ("already_present", Json::Bool(false)),
                    ("message", Json::Null),
                ])
            };
            Ok(Json::object([
                ("workspace", workspace.to_json()),
                ("git_export", git_export),
            ])
            .encode())
        }
    }

    #[tauri::command(async)]
    fn remember_managed_workspace(
        runtime: State<'_, DesktopRuntime>,
        path: String,
        export_root: Option<String>,
        original_update_version: Option<String>,
    ) -> Result<String, String> {
        let open = runtime
            .daemon
            .workspace_state()
            .map_err(|error| error.to_string())?;
        if open.root != path {
            return Err(
                "Only the workspace currently validated by Mesh can be remembered".to_owned(),
            );
        }
        runtime
            .recent
            .ensure_can_remember(std::path::Path::new(&path))
            .map_err(|error| error.to_string())?;
        if let Some(version) = original_update_version.as_deref() {
            if open.shared_version.as_deref() != Some(version) {
                return Err(
                    "Only the exact current shared version can complete an original-folder update"
                        .to_owned(),
                );
            }
            runtime
                .recent
                .record_original_update(std::path::Path::new(&path), version)
                .map_err(|error| error.to_string())?;
        }
        let status = remember_current_workspace_navigation(
            &runtime.daemon,
            &runtime.recent,
            &runtime.active_workspace,
            &open,
            export_root.as_deref().map(std::path::Path::new),
        )?;
        *runtime
            .recent_status
            .lock()
            .map_err(|_| "The recent workspace status is unavailable".to_owned())? = status.clone();
        Ok(status.to_json())
    }

    #[tauri::command(async)]
    fn forget_managed_workspace(
        runtime: State<'_, DesktopRuntime>,
        path: String,
    ) -> Result<String, String> {
        let status = forget_workspace_navigation(
            &runtime.daemon,
            &runtime.recent,
            &runtime.active_workspace,
            std::path::Path::new(&path),
        )?;
        *runtime
            .recent_status
            .lock()
            .map_err(|_| "The recent workspace status is unavailable".to_owned())? = status.clone();
        Ok(status.to_json())
    }

    #[cfg(target_os = "macos")]
    const NATIVE_FOLDER_OPENER: &str = "/usr/bin/open";
    #[cfg(target_os = "macos")]
    const FINDER_APPLICATION_PATH: &str = "/System/Library/CoreServices/Finder.app";
    #[cfg(target_os = "linux")]
    const NATIVE_FOLDER_OPENER: &str = "/usr/bin/xdg-open";
    #[cfg(target_os = "windows")]
    const NATIVE_FOLDER_OPENER: &str = r"C:\Windows\explorer.exe";
    const NATIVE_FOLDER_OPEN_TIMEOUT: Duration = Duration::from_secs(5);
    #[cfg(target_os = "macos")]
    const CODEX_BUNDLE_IDENTIFIER: &str = "com.openai.codex";
    #[cfg(any(target_os = "macos", test))]
    const TERMINAL_BUNDLE_IDENTIFIER: &str = "com.apple.Terminal";

    #[cfg(target_os = "macos")]
    #[link(name = "CoreServices", kind = "framework")]
    #[allow(unsafe_code)]
    unsafe extern "C" {
        fn LSCopyDefaultApplicationURLForURL(
            url: core_foundation::url::CFURLRef,
            roles: u32,
            error: *mut *const std::ffi::c_void,
        ) -> core_foundation::url::CFURLRef;
    }

    fn wait_for_launcher(
        mut child: std::process::Child,
        timeout: Duration,
        nonzero_message: &str,
        timeout_message: &str,
        result_context: &str,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) => return Err(nonzero_message.to_owned()),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(timeout_message.to_owned());
                }
                Err(error) => {
                    return Err(format!("{result_context}: {error}"));
                }
            }
        }
    }

    fn open_native_folder_with(
        program: &std::path::Path,
        path: &std::path::Path,
        timeout: Duration,
    ) -> Result<(), String> {
        let child = Command::new(program)
            .arg(path)
            .spawn()
            .map_err(|error| format!("The working-folder opener could not start: {error}"))?;
        wait_for_launcher(
            child,
            timeout,
            "The operating system could not open the working folder. Open the stable path manually or try again.",
            "The working-folder opener did not finish. Open the stable path manually or try again.",
            "The working-folder opener result was unavailable",
        )
    }

    fn open_native_folder(path: &std::path::Path) -> Result<(), String> {
        open_native_folder_with(
            std::path::Path::new(NATIVE_FOLDER_OPENER),
            path,
            NATIVE_FOLDER_OPEN_TIMEOUT,
        )
    }

    fn stable_native_reference(path: &Path, directory: bool) -> Result<PathBuf, String> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("Mesh could not inspect the exact review copy: {error}"))?;
        let expected_kind = if directory {
            metadata.file_type().is_dir()
        } else {
            metadata.file_type().is_file()
        };
        if !expected_kind || metadata.file_type().is_symlink() {
            return Err("The exact review copy changed before it could be opened.".to_owned());
        }
        #[cfg(target_os = "macos")]
        {
            let reference = PathBuf::from(format!("/.vol/{}/{}", metadata.dev(), metadata.ino()));
            let stable = fs::symlink_metadata(&reference).map_err(|error| {
                format!("Mesh could not bind the exact review copy to a stable reference: {error}")
            })?;
            let stable_kind = if directory {
                stable.file_type().is_dir()
            } else {
                stable.file_type().is_file()
            };
            if stable_kind
                && !stable.file_type().is_symlink()
                && stable.dev() == metadata.dev()
                && stable.ino() == metadata.ino()
            {
                Ok(reference)
            } else {
                Err(
                    "The stable review-copy reference did not match the exact saved bytes."
                        .to_owned(),
                )
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(path.to_path_buf())
        }
    }

    fn safe_review_text_extension(path: &str, bytes: &[u8]) -> Option<&'static str> {
        if std::str::from_utf8(bytes).is_err() {
            return None;
        }
        match Path::new(path)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("md") => Some("md"),
            Some("json") => Some("json"),
            Some("yaml") => Some("yaml"),
            Some("yml") => Some("yml"),
            Some("toml") => Some("toml"),
            Some("csv") => Some("csv"),
            Some("log") => Some("log"),
            Some("rs") => Some("rs"),
            Some("js") => Some("js"),
            Some("jsx") => Some("jsx"),
            Some("ts") => Some("ts"),
            Some("tsx") => Some("tsx"),
            Some("css") => Some("css"),
            Some("c") => Some("c"),
            Some("cc") => Some("cc"),
            Some("cpp") => Some("cpp"),
            Some("h") => Some("h"),
            Some("hpp") => Some("hpp"),
            Some("py") => Some("py"),
            Some("rb") => Some("rb"),
            Some("go") => Some("go"),
            Some("java") => Some("java"),
            Some("swift") => Some("swift"),
            Some("kt") => Some("kt"),
            _ => Some("txt"),
        }
    }

    fn safe_review_image_extension(path: &str, bytes: &[u8]) -> Option<&'static str> {
        match Path::new(path)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("png") if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Some("png"),
            Some("jpg" | "jpeg") if bytes.starts_with(&[0xff, 0xd8, 0xff]) => Some("jpg"),
            Some("gif") if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => {
                Some("gif")
            }
            Some("webp")
                if bytes.len() >= 12
                    && bytes.starts_with(b"RIFF")
                    && bytes.get(8..12) == Some(b"WEBP") =>
            {
                Some("webp")
            }
            _ => None,
        }
    }

    fn admitted_review_inspection_extension(
        path: &str,
        bytes: &[u8],
        version: &str,
        digest: &str,
        expected_version: &str,
        expected_digest: &str,
        action: &str,
    ) -> Result<&'static str, String> {
        if version != expected_version || digest != expected_digest {
            return Err(
                "The saved side changed before the native action began. Refresh Review and choose the exact side again."
                    .to_owned(),
            );
        }
        let safe_open_extension = inspection_extension(path, bytes)
            .or_else(|| safe_review_image_extension(path, bytes))
            .or_else(|| safe_review_text_extension(path, bytes));
        if action == "open-entry" && safe_open_extension.is_none() {
            return Err(
                "This exact binary copy cannot be opened safely by default. Use Reveal in Finder or Open copy folder instead."
                    .to_owned(),
            );
        }
        Ok(safe_open_extension.unwrap_or("bin"))
    }

    fn live_raster_data_url(path: &str, bytes: &[u8]) -> Option<String> {
        if bytes.len() > 8 * 1024 * 1024 {
            return None;
        }
        let extension = Path::new(path)
            .extension()
            .and_then(std::ffi::OsStr::to_str)?
            .to_ascii_lowercase();
        let mime = match extension.as_str() {
            "png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => "image/png",
            "jpg" | "jpeg" if bytes.starts_with(&[0xff, 0xd8, 0xff]) => "image/jpeg",
            "gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => "image/gif",
            "webp"
                if bytes.len() >= 12
                    && bytes.starts_with(b"RIFF")
                    && bytes.get(8..12) == Some(b"WEBP") =>
            {
                "image/webp"
            }
            _ => return None,
        };
        Some(format!("data:{mime};base64,{}", encode_base64(bytes)))
    }

    fn open_native_entry_with(
        program: &std::path::Path,
        reference: &std::path::Path,
        action: &str,
        default_application: Option<&std::path::Path>,
        timeout: Duration,
    ) -> Result<(), String> {
        let mut command = Command::new(program);
        match action {
            "open-entry" => {
                if let Some(application) = default_application {
                    command.arg("-a").arg(application);
                }
            }
            "reveal-entry" => {
                if default_application.is_some() {
                    return Err("Finder reveal cannot name a document application".to_owned());
                }
                #[cfg(target_os = "macos")]
                command.arg("-R");
            }
            _ => return Err("The workspace-entry action was not recognized".to_owned()),
        }
        let child = command
            .arg(reference)
            .spawn()
            .map_err(|error| format!("The workspace-entry launcher could not start: {error}"))?;
        wait_for_launcher(
            child,
            timeout,
            "The operating system could not open this workspace entry. Try again or open the workspace folder.",
            "The workspace-entry launcher did not finish. Try again or open the workspace folder.",
            "The workspace-entry launcher result was unavailable",
        )
    }

    #[cfg(target_os = "macos")]
    #[allow(unsafe_code)]
    fn default_native_application(path: &Path) -> Result<PathBuf, String> {
        let url = CFURL::from_path(path, false).ok_or_else(|| {
            "The exact saved copy could not be represented as a file URL.".to_owned()
        })?;
        // SAFETY: LaunchServices borrows the valid CFURL for this call. Passing no error output
        // avoids an additional owned CF object. A non-null result follows the Create rule and is
        // immediately wrapped so it is released after its filesystem path is copied.
        let application = unsafe {
            LSCopyDefaultApplicationURLForURL(
                url.as_concrete_TypeRef(),
                u32::MAX,
                std::ptr::null_mut(),
            )
        };
        if application.is_null() {
            return Err(
                "No default application is registered for this exact saved file type.".to_owned(),
            );
        }
        // SAFETY: the non-null result is owned under LaunchServices' documented Create rule.
        let application = unsafe { CFURL::wrap_under_create_rule(application) };
        let path = application.to_path().ok_or_else(|| {
            "The default application did not resolve to a local application bundle.".to_owned()
        })?;
        if !path.is_absolute() || !path.is_dir() {
            return Err(
                "The default application did not resolve to a local application bundle.".to_owned(),
            );
        }
        Ok(path)
    }

    #[cfg(any(target_os = "macos", test))]
    fn open_codex_workspace_with(
        program: &std::path::Path,
        path: &std::path::Path,
        overrides: &[String],
        timeout: Duration,
    ) -> Result<(), String> {
        let mut command = Command::new(program);
        command.arg("app");
        for value in overrides {
            command.arg("-c").arg(value);
        }
        let child = command
            .arg(path)
            .spawn()
            .map_err(|error| format!("Codex could not be started: {error}"))?;
        wait_for_launcher(
            child,
            timeout,
            "Codex could not open this workspace. Install or reopen the Codex desktop app, then try again.",
            "Codex did not finish accepting this workspace. Open the exact workspace path in Codex manually or try again.",
            "The Codex launcher result was unavailable",
        )
    }

    #[cfg(not(target_os = "macos"))]
    fn claude_cli_path() -> Option<PathBuf> {
        None
    }

    #[cfg(target_os = "macos")]
    fn claude_cli_path() -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join(".local/bin/claude"));
        }
        candidates.extend([
            PathBuf::from("/opt/homebrew/bin/claude"),
            PathBuf::from("/usr/local/bin/claude"),
        ]);
        // Claude's native installation commonly uses a versioned symlink. Resolve it natively;
        // the adapter still verifies that the resolved absolute path is an executable file.
        candidates
            .into_iter()
            .filter_map(|path| path.canonicalize().ok())
            .find(|path| path.is_file())
    }

    #[cfg(not(target_os = "macos"))]
    fn codex_cli_path() -> Option<PathBuf> {
        None
    }

    #[cfg(target_os = "macos")]
    fn codex_cli_path() -> Option<PathBuf> {
        let mut candidates = vec![
            PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex"),
            PathBuf::from("/Applications/Codex.app/Contents/Resources/codex"),
        ];
        if let Some(home) = std::env::var_os("HOME") {
            let applications = PathBuf::from(home).join("Applications");
            candidates.push(applications.join("ChatGPT.app/Contents/Resources/codex"));
            candidates.push(applications.join("Codex.app/Contents/Resources/codex"));
        }
        candidates.into_iter().find(|candidate| {
            fs::symlink_metadata(candidate).is_ok_and(|metadata| metadata.file_type().is_file())
        })
    }

    #[cfg(target_os = "macos")]
    fn open_codex_workspace(path: &std::path::Path, overrides: &[String]) -> Result<bool, String> {
        if let Some(cli) = codex_cli_path() {
            open_codex_workspace_with(&cli, path, overrides, NATIVE_FOLDER_OPEN_TIMEOUT)?;
            return Ok(true);
        }
        let child = Command::new("/usr/bin/open")
            .arg("-b")
            .arg(CODEX_BUNDLE_IDENTIFIER)
            .arg(path)
            .spawn()
            .map_err(|error| format!("Codex could not be started: {error}"))?;
        wait_for_launcher(
            child,
            NATIVE_FOLDER_OPEN_TIMEOUT,
            "Codex could not open this workspace. Install or reopen the Codex desktop app, then try again.",
            "Codex did not finish accepting this workspace. Open the exact workspace path in Codex manually or try again.",
            "The Codex launcher result was unavailable",
        )?;
        Ok(false)
    }

    #[derive(Debug, Eq, PartialEq)]
    struct CodexContextOutcome {
        state: &'static str,
        warning: Option<String>,
    }

    /// The project-scoped Mesh tool is optional context, never authority to open the native
    /// workspace. In particular, a project that already owns `.codex` must keep those bytes and
    /// must still be handed to Codex normally.
    fn open_codex_after_optional_context(
        path: &std::path::Path,
        context: Result<CodexProjectConfig, CodexProjectConfigError>,
        overrides: Result<Vec<String>, CodexProjectConfigError>,
        launch: impl FnOnce(&std::path::Path, &[String]) -> Result<bool, String>,
    ) -> Result<CodexContextOutcome, String> {
        let launch_overrides = overrides.as_deref().unwrap_or(&[]);
        let launch_scoped_context = launch(path, launch_overrides)?;
        let outcome = match (launch_scoped_context, overrides, context) {
            (true, Ok(_), Ok(CodexProjectConfig::Created)) => CodexContextOutcome {
                state: "installed",
                warning: None,
            },
            (true, Ok(_), Ok(CodexProjectConfig::Refreshed)) => CodexContextOutcome {
                state: "refreshed",
                warning: None,
            },
            (true, Ok(_), _) => CodexContextOutcome {
                state: "ready",
                warning: None,
            },
            (false, _, _) => CodexContextOutcome {
                state: "unavailable",
                warning: Some(
                    "Codex opened, but Mesh could not find a Codex CLI that supports launch-scoped context. The workspace remains usable without the optional Mesh tool."
                        .to_owned(),
                ),
            },
            (true, Err(error), _) => CodexContextOutcome {
                state: "unavailable",
                warning: Some(error.to_string()),
            },
        };
        Ok(outcome)
    }

    #[cfg(not(target_os = "macos"))]
    fn open_codex_workspace(
        _path: &std::path::Path,
        _overrides: &[String],
    ) -> Result<bool, String> {
        Err("Start Codex on this version is available in this alpha on macOS. Open the exact workspace path in your agent manually on this platform.".to_owned())
    }

    #[cfg(any(target_os = "macos", test))]
    fn open_terminal_workspace_with(
        program: &std::path::Path,
        bundle_identifier: &str,
        path: &std::path::Path,
        timeout: Duration,
    ) -> Result<(), String> {
        let child = Command::new(program)
            .arg("-b")
            .arg(bundle_identifier)
            .arg(path)
            .spawn()
            .map_err(|error| format!("Terminal could not be started: {error}"))?;
        wait_for_launcher(
            child,
            timeout,
            "Terminal could not open this workspace. Reopen Terminal, then try again.",
            "Terminal did not finish accepting this workspace. Copy the exact agent path and open it manually or try again.",
            "The Terminal launcher result was unavailable",
        )
    }

    #[cfg(target_os = "macos")]
    fn open_terminal_workspace(path: &std::path::Path) -> Result<(), String> {
        open_terminal_workspace_with(
            std::path::Path::new("/usr/bin/open"),
            TERMINAL_BUNDLE_IDENTIFIER,
            path,
            NATIVE_FOLDER_OPEN_TIMEOUT,
        )
    }

    #[cfg(not(target_os = "macos"))]
    fn open_terminal_workspace(_path: &std::path::Path) -> Result<(), String> {
        Err("Open agent terminal is available in this alpha on macOS. Copy the exact agent path and open it in your terminal on this platform.".to_owned())
    }

    // Filesystem launchers may wait for the operating system. Dispatch the synchronous body on
    // Tauri's async command executor so the macOS application event thread keeps painting the
    // in-progress state and remains interactive while the verified folder opens.
    #[tauri::command(async)]
    fn reveal_managed_workspace(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let verified = runtime
            .daemon
            .verified_managed_workspace_path(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
            )
            .map_err(|error| error.to_string())?;
        if !verified.is_presented() {
            runtime
                .active_workspace
                .deactivate()
                .map_err(|error| error.to_string())?;
            return Err("This older workspace layout has no isolated native folder. Under Workspace versions, open its current saved workspace as a new folder first.".to_owned());
        }
        let stable_path = activate_verified_native_folder(&runtime.active_workspace, &verified)?;
        open_native_folder(&stable_path)?;
        Ok(Json::object([
            ("path", Json::text(stable_path.display().to_string())),
            (
                "workspace_root",
                Json::text(verified.path().display().to_string()),
            ),
            ("stable", Json::Bool(true)),
            ("native_folder", Json::Bool(true)),
        ])
        .encode())
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    fn open_managed_workspace_entry(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        expected_agent_handoff_generation: Option<String>,
        relative_path: String,
        entry_kind: String,
        action: String,
    ) -> Result<String, String> {
        if let Some(generation) = expected_agent_handoff_generation.as_deref() {
            parse_agent_handoff_generation(generation)?;
        }
        let is_directory = match entry_kind.as_str() {
            "file" => false,
            "folder" => true,
            _ => return Err("The selected workspace entry kind was not recognized".to_owned()),
        };
        if action != "open-entry" && action != "reveal-entry" {
            return Err("The workspace-entry action was not recognized".to_owned());
        }
        if is_directory && action == "reveal-entry" {
            return Err("Folders are opened in Finder rather than revealed as files".to_owned());
        }
        let entry = runtime
            .daemon
            .verified_managed_workspace_entry(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                expected_agent_handoff_generation.as_deref(),
                &relative_path,
                is_directory,
            )
            .map_err(|error| error.to_string())?;
        entry.ensure_current().map_err(|error| {
            format!("The selected workspace entry changed before it could be opened: {error}")
        })?;
        #[cfg(target_os = "macos")]
        let launch_reference = entry.stable_reference().map_err(|error| {
            format!(
                "The selected workspace entry could not be bound to a stable reference: {error}"
            )
        })?;
        #[cfg(not(target_os = "macos"))]
        let launch_reference = entry.path().to_path_buf();
        #[cfg(target_os = "macos")]
        let default_application = if action == "open-entry" {
            if is_directory {
                Some(PathBuf::from(FINDER_APPLICATION_PATH))
            } else {
                Some(default_native_application(entry.path())?)
            }
        } else {
            None
        };
        #[cfg(not(target_os = "macos"))]
        let default_application: Option<PathBuf> = None;
        open_native_entry_with(
            std::path::Path::new(NATIVE_FOLDER_OPENER),
            &launch_reference,
            &action,
            default_application.as_deref(),
            NATIVE_FOLDER_OPEN_TIMEOUT,
        )?;
        Ok(Json::object([
            ("schema", Json::text("mesh.workspace-entry-open/v1")),
            ("action", Json::text(action)),
            ("entry_kind", Json::text(entry_kind)),
            ("opened", Json::Bool(true)),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn reconcile_managed_workspace_navigation(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let (stable, real) = reconcile_verified_native_folder(
            &runtime.daemon,
            &runtime.active_workspace,
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
        )?;
        let mut recent_status = runtime
            .recent_status
            .lock()
            .map_err(|_| "The recent workspace status is unavailable".to_owned())?;
        recent_status.active_folder = stable.as_ref().map(|path| path.display().to_string());
        Ok(Json::object([
            (
                "path",
                stable
                    .as_ref()
                    .map_or(Json::Null, |path| Json::text(path.display().to_string())),
            ),
            ("workspace_root", Json::text(real.display().to_string())),
            ("stable", Json::Bool(stable.is_some())),
            ("native_folder", Json::Bool(stable.is_some())),
        ])
        .encode())
    }

    /// Reconcile the stable native folder from the daemon's currently held workspace.
    ///
    /// This is the fail-safe counterpart to the caller-bound reconciliation above. The browser
    /// uses it only after a workspace-changing operation completed but the follow-up workspace or
    /// checkpoint read failed. At that point the older browser snapshot cannot name the daemon's
    /// new root safely, so the native host derives the current verified target itself. A workspace
    /// that cannot be verified deactivates the stable link rather than leaving it on the prior
    /// version.
    #[tauri::command(async)]
    fn reconcile_current_workspace_navigation(
        runtime: State<'_, DesktopRuntime>,
    ) -> Result<String, String> {
        match reconcile_active_native_folder(&runtime.daemon, &runtime.active_workspace) {
            Ok(active) => Ok(Json::object([
                (
                    "path",
                    active
                        .as_ref()
                        .map_or(Json::Null, |path| Json::text(path.display().to_string())),
                ),
                ("stable", Json::Bool(active.is_some())),
                ("warning", Json::Null),
            ])
            .encode()),
            Err(warning) => Ok(Json::object([
                ("path", Json::Null),
                ("stable", Json::Bool(false)),
                ("warning", Json::text(warning)),
            ])
            .encode()),
        }
    }

    fn refresh_agent_handoff_navigation(
        recent: &RecentWorkspace,
        recent_status: &Mutex<RecentWorkspaceStatus>,
    ) -> Result<(), String> {
        // The durable record is shared by every Mesh process. Refreshing only the custody fields
        // leaves a long-running window's Recent workspaces list stale after another process opens
        // or forgets a workspace, even though the same read already returned the authoritative
        // bounded order. Preserve this process's active stable link and export selection, but
        // replace every shared navigation field together from one record snapshot.
        let entries = recent.load_entries().map_err(|error| {
            format!("Mesh could not verify its agent-folder record before opening it: {error}")
        })?;
        let workspaces: Vec<_> = entries
            .iter()
            .map(|entry| entry.path().display().to_string())
            .collect();
        let workspace_entries = recent_navigation_entries(&entries);
        let mut status = recent_status
            .lock()
            .map_err(|_| "The recent workspace status is unavailable".to_owned())?;
        status.remembered = workspaces.first().cloned();
        status.workspaces = workspaces;
        status.workspace_entries = workspace_entries;
        Ok(())
    }

    fn reconcile_current_agent_handoff(
        daemon: &LiveDaemon,
        recent: &RecentWorkspace,
    ) -> Result<(), String> {
        let summary = match daemon.current_workspace_summary() {
            Ok(summary) => summary,
            Err(_) => return Ok(()),
        };
        let verified = daemon
            .verified_managed_workspace_path(&summary.root, &summary.digest, &summary.installation)
            .map_err(|error| format!("Mesh could not verify the current agent folder: {error}"))?;
        recent
            .reconcile_agent_handoff_from_authority(
                verified.path(),
                &summary.installation,
                &verified.directory_token(),
                |projected_active| {
                    daemon
                        .reconcile_workspace_agent_custody(
                            &summary.root,
                            &summary.digest,
                            &summary.installation,
                            projected_active,
                        )
                        .map(|custody| custody.generation().map(str::to_owned))
                        .map_err(|error| {
                            crate::recent_workspace::RecentWorkspaceError::Authority(format!(
                                "Mesh could not read workspace agent custody: {error}"
                            ))
                        })
                },
            )
            .map_err(|error| format!("Mesh could not reconcile its agent-folder record: {error}"))
    }

    struct AgentHandoffBinding<'a> {
        path: &'a std::path::Path,
        installation: &'a str,
        directory: String,
        root: &'a str,
        digest: &'a str,
    }

    fn persist_agent_handoff(
        daemon: &LiveDaemon,
        recent: &RecentWorkspace,
        recent_status: &Mutex<RecentWorkspaceStatus>,
        binding: AgentHandoffBinding<'_>,
        confirmed_reopen: bool,
        expected_generation: Option<&str>,
    ) -> Result<String, String> {
        let generation = recent
            .record_agent_handoff_with_authority(
                binding.path,
                binding.installation,
                &binding.directory,
                confirmed_reopen,
                expected_generation,
                || {
                    daemon
                        .acquire_workspace_agent_custody(
                            binding.root,
                            binding.digest,
                            binding.installation,
                            confirmed_reopen,
                            expected_generation,
                        )
                        .map_err(|error| {
                            crate::recent_workspace::RecentWorkspaceError::Authority(format!(
                                "Mesh could not acquire workspace agent custody: {error}"
                            ))
                        })
                },
            )
            .map_err(|error| {
                format!(
                    "Mesh could not durably record this agent folder before opening it: {error}"
                )
            })?;
        refresh_agent_handoff_navigation(recent, recent_status)?;
        Ok(generation)
    }

    fn parse_agent_handoff_generation(value: &str) -> Result<&str, String> {
        let canonical = value == "legacy-v8"
            || (value.len() == 32
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        if !canonical {
            return Err("The agent-folder handoff generation is not a canonical token".to_owned());
        }
        Ok(value)
    }

    /// Run one managed working-copy mutation only while process-shared agent custody is absent.
    ///
    /// The recent-workspace guard must be acquired before the daemon enters its verified workspace,
    /// checkpoint, or managed-edit locks. Agent handoff acquisition takes the same native file lock,
    /// making a stale renderer harmless: either this mutation finishes first or custody is already
    /// durable and the mutation closure is never entered.
    fn with_unassigned_managed_workspace<T>(
        recent: &RecentWorkspace,
        expected_workspace_root: &str,
        expected_workspace_installation: &str,
        mutation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _custody = recent
            .lock_managed_workspace_mutation(
                std::path::Path::new(expected_workspace_root),
                expected_workspace_installation,
            )
            .map_err(|error| format!("Mesh refused the managed change: {error}"))?;
        mutation()
    }

    /// Verify and durably assign the exact native folder before exposing its path for a manual
    /// agent handoff. Copying a path is the same collision boundary as launching Codex or an agent
    /// terminal: once another process may receive the writable directory, later handoffs must
    /// require an explicit release or a fresh independent copy.
    #[tauri::command(async)]
    fn prepare_managed_workspace_agent_path(
        runtime: State<'_, DesktopRuntime>,
        expected_agent_path: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        confirmed_reopen: bool,
        expected_agent_handoff_generation: Option<String>,
    ) -> Result<String, String> {
        let verified = runtime
            .daemon
            .verified_managed_workspace_path(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
            )
            .map_err(|error| error.to_string())?;
        if !verified.is_presented() {
            return Err("This older workspace layout has no isolated agent folder. Open its current saved workspace as a new folder first.".to_owned());
        }
        verified
            .ensure_current()
            .map_err(|error| error.to_string())?;
        if verified.path().to_str() != Some(expected_agent_path.as_str()) {
            return Err("The displayed agent folder no longer matches the verified native workspace. Refresh before copying it.".to_owned());
        }
        let agent_reference = verified.stable_agent_reference().map_err(|error| {
            format!("Mesh could not create a stable reference for this agent folder: {error}")
        })?;
        let git_source = git_context_source_for_path(&runtime.recent, verified.path());
        let git_destination_identity = git_setup_destination_identity(verified.path())?;
        let handoff_generation = persist_agent_handoff(
            &runtime.daemon,
            &runtime.recent,
            &runtime.recent_status,
            AgentHandoffBinding {
                path: verified.path(),
                installation: &expected_workspace_installation,
                directory: verified.directory_token(),
                root: &expected_workspace_root,
                digest: &expected_workspace_digest,
            },
            confirmed_reopen,
            expected_agent_handoff_generation.as_deref(),
        )?;
        let (git_context, git_context_warning) = with_agent_setup_authority(
            &runtime.daemon,
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
            &handoff_generation,
            || {
                let git = ensure_git_context_for_source(
                    &agent_reference,
                    &git_source,
                    &git_destination_identity,
                );
                verified.ensure_current().map_err(|error| format!(
                    "The workspace changed while Mesh was preparing its independent Git context. Refresh before copying it: {error}"
                ))?;
                Ok(git)
            },
        )?;
        Ok(Json::object([
            ("path", Json::text(agent_reference.display().to_string())),
            (
                "display_path",
                Json::text(verified.path().display().to_string()),
            ),
            (
                "workspace_installation",
                Json::text(expected_workspace_installation),
            ),
            ("agent_handoff_recorded", Json::Bool(true)),
            ("agent_handoff_generation", Json::text(&handoff_generation)),
            ("git_context", Json::text(git_context)),
            (
                "git_context_warning",
                git_context_warning.map_or(Json::Null, Json::text),
            ),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn open_managed_workspace_in_codex(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        confirmed_reopen: bool,
        expected_agent_handoff_generation: Option<String>,
    ) -> Result<String, String> {
        let verified = runtime
            .daemon
            .verified_managed_workspace_path(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
            )
            .map_err(|error| error.to_string())?;
        if !verified.is_presented() {
            return Err("This older workspace layout has no isolated native folder. Open its current saved workspace as a new folder before starting an agent.".to_owned());
        }
        verified
            .ensure_current()
            .map_err(|error| error.to_string())?;
        // Recent is an app-owned source hint and has its own process-shared lock. Read it before
        // acquiring workspace-native custody so native setup never introduces Shared -> Recent.
        let git_source = git_context_source_for_path(&runtime.recent, verified.path());
        let git_destination_identity = git_setup_destination_identity(verified.path())?;
        let agent_reference = verified.stable_agent_reference().map_err(|error| {
            format!("Mesh could not create a stable reference for this Codex workspace: {error}")
        })?;
        let storage_reference = stable_storage_reference(verified.path())?;
        let executable = std::env::current_exe()
            .and_then(fs::canonicalize)
            .map_err(|error| format!("Mesh could not verify its installed executable: {error}"))?;
        let launch_overrides = codex_launch_overrides(
            &executable,
            &runtime.endpoint,
            verified.path(),
            &expected_workspace_installation,
        );
        // Record a conservative collision warning before starting an external process. If the
        // launcher fails or this process crashes, an extra confirmation is safe; forgetting a
        // launcher that may have succeeded is not. This app-owned hint never authorizes workspace
        // access and is bound to the daemon-verified physical installation.
        let handoff_generation = persist_agent_handoff(
            &runtime.daemon,
            &runtime.recent,
            &runtime.recent_status,
            AgentHandoffBinding {
                path: verified.path(),
                installation: &expected_workspace_installation,
                directory: verified.directory_token(),
                root: &expected_workspace_root,
                digest: &expected_workspace_digest,
            },
            confirmed_reopen,
            expected_agent_handoff_generation.as_deref(),
        )?;
        // Setup and launcher acceptance stay under the exact generation's shared custody lock.
        // A concurrent release or stale second window therefore cannot cross a `.git`/`.codex`
        // write or let the launcher start after its assignment was already cleared.
        let ((git_context, git_context_warning), context) = with_agent_setup_authority(
            &runtime.daemon,
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
            &handoff_generation,
            || {
                let git = ensure_git_context_for_source(
                    &agent_reference,
                    &git_source,
                    &git_destination_identity,
                );
                verified.ensure_current().map_err(|error| format!(
                    "The workspace changed while Mesh was preparing its independent Git context. Verify the current Mesh version before opening Codex: {error}"
                ))?;
                let context = open_codex_after_optional_context(
                    &agent_reference,
                    ensure_codex_project_config_at_references(
                        verified.path(),
                        &agent_reference,
                        &storage_reference,
                        &executable,
                        &runtime.endpoint,
                        &expected_workspace_installation,
                    ),
                    launch_overrides,
                    |path, overrides| match runtime.renderer_proof.accept_agent_handoff_launch(
                        verified.path(),
                        &expected_workspace_installation,
                        &handoff_generation,
                    )? {
                        Some(accepted) => Ok(accepted),
                        None => open_codex_workspace(path, overrides),
                    },
                )?;
                verified.ensure_current().map_err(|error| format!(
                    "The workspace changed while Codex was opening it. Close that Codex workspace and verify the current Mesh version before trying again: {error}"
                ))?;
                Ok((git, context))
            },
        )?;
        Ok(Json::object([
            ("path", Json::text(verified.path().display().to_string())),
            (
                "workspace_installation",
                Json::text(expected_workspace_installation),
            ),
            ("fixed_workspace_path", Json::Bool(true)),
            ("agent_handoff_recorded", Json::Bool(true)),
            ("agent_handoff_generation", Json::text(&handoff_generation)),
            ("agent", Json::text("Codex")),
            ("mesh_context", Json::text(context.state)),
            (
                "mesh_context_warning",
                context.warning.map_or(Json::Null, Json::text),
            ),
            ("git_context", Json::text(git_context)),
            (
                "git_context_warning",
                git_context_warning.map_or(Json::Null, Json::text),
            ),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn open_managed_workspace_in_terminal(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        confirmed_reopen: bool,
        expected_agent_handoff_generation: Option<String>,
    ) -> Result<String, String> {
        let verified = runtime
            .daemon
            .verified_managed_workspace_path(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
            )
            .map_err(|error| error.to_string())?;
        if !verified.is_presented() {
            return Err("This older workspace layout has no isolated native folder. Open its current saved workspace as a new folder before starting an agent terminal.".to_owned());
        }
        verified
            .ensure_current()
            .map_err(|error| error.to_string())?;
        let git_source = git_context_source_for_path(&runtime.recent, verified.path());
        let git_destination_identity = git_setup_destination_identity(verified.path())?;
        let agent_reference = verified.stable_agent_reference().map_err(|error| {
            format!("Mesh could not create a stable reference for this Terminal workspace: {error}")
        })?;
        // Opening an agent terminal is the same writable-folder handoff boundary as opening
        // Codex. Record it before invoking the external launcher so a later process or app
        // restart cannot silently hand this exact installation to a second agent.
        let handoff_generation = persist_agent_handoff(
            &runtime.daemon,
            &runtime.recent,
            &runtime.recent_status,
            AgentHandoffBinding {
                path: verified.path(),
                installation: &expected_workspace_installation,
                directory: verified.directory_token(),
                root: &expected_workspace_root,
                digest: &expected_workspace_digest,
            },
            confirmed_reopen,
            expected_agent_handoff_generation.as_deref(),
        )?;
        let (git_context, git_context_warning) = with_agent_setup_authority(
            &runtime.daemon,
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
            &handoff_generation,
            || {
                let git = ensure_git_context_for_source(
                    &agent_reference,
                    &git_source,
                    &git_destination_identity,
                );
                verified.ensure_current().map_err(|error| format!(
                    "The workspace changed while Mesh was preparing its independent Git context. Verify the current Mesh version before opening Terminal: {error}"
                ))?;
                open_terminal_workspace(&agent_reference)?;
                verified.ensure_current().map_err(|error| format!(
                    "The workspace changed while Terminal was opening it. Close that Terminal window and verify the current Mesh version before trying again: {error}"
                ))?;
                Ok(git)
            },
        )?;
        Ok(Json::object([
            ("path", Json::text(verified.path().display().to_string())),
            (
                "workspace_installation",
                Json::text(expected_workspace_installation),
            ),
            ("fixed_workspace_path", Json::Bool(true)),
            ("agent_handoff_recorded", Json::Bool(true)),
            ("agent_handoff_generation", Json::text(&handoff_generation)),
            ("agent", Json::text("Terminal")),
            ("git_context", Json::text(git_context)),
            (
                "git_context_warning",
                git_context_warning.map_or(Json::Null, Json::text),
            ),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn finish_managed_workspace_agent_handoff(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        expected_agent_handoff_generation: String,
    ) -> Result<String, String> {
        parse_agent_handoff_generation(&expected_agent_handoff_generation)?;
        let verified = runtime
            .daemon
            .verified_managed_workspace_path(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
            )
            .map_err(|error| error.to_string())?;
        if !verified.is_presented() {
            return Err("This older workspace layout has no isolated agent folder to release. Open its current saved workspace as a new folder first.".to_owned());
        }
        verified
            .ensure_current()
            .map_err(|error| error.to_string())?;
        let cleared = runtime
            .recent
            .clear_agent_handoff_with_authority(
                verified.path(),
                &expected_workspace_installation,
                &expected_agent_handoff_generation,
                || {
                    runtime
                        .daemon
                        .release_workspace_agent_custody(
                            &expected_workspace_root,
                            &expected_workspace_digest,
                            &expected_workspace_installation,
                            &expected_agent_handoff_generation,
                        )
                        .map_err(|error| {
                            crate::recent_workspace::RecentWorkspaceError::Authority(format!(
                                "Mesh could not release workspace agent custody: {error}"
                            ))
                        })
                },
            )
            .map_err(|error| {
                format!("Mesh could not clear this exact agent-folder handoff: {error}")
            })?;
        refresh_agent_handoff_navigation(&runtime.recent, &runtime.recent_status)?;
        verified.ensure_current().map_err(|error| format!(
            "The workspace changed while Mesh was clearing its agent-folder handoff. Refresh before starting another agent: {error}"
        ))?;
        runtime.renderer_proof.record_agent_handoff_release(
            &expected_workspace_root,
            &expected_workspace_installation,
            &expected_agent_handoff_generation,
        )?;
        Ok(Json::object([
            ("path", Json::text(verified.path().display().to_string())),
            (
                "workspace_installation",
                Json::text(expected_workspace_installation),
            ),
            ("cleared", Json::Bool(cleared)),
        ])
        .encode())
    }

    #[derive(Debug)]
    struct PreparedGitContext {
        #[cfg(feature = "git-integration")]
        anchor: Option<mesh_git_bridge::GitProvenanceAnchor>,
        warning: Option<String>,
    }

    fn prepare_git_context(source: &std::path::Path) -> PreparedGitContext {
        #[cfg(feature = "git-integration")]
        {
            match fs::symlink_metadata(source.join(".git")) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => PreparedGitContext {
                    anchor: None,
                    warning: None,
                },
                Err(error) => PreparedGitContext {
                    anchor: None,
                    warning: Some(format!(
                        "Mesh could not inspect this folder's Git metadata: {error}"
                    )),
                },
                Ok(_) => match mesh_git_bridge::GitProvenanceAnchor::inspect(source) {
                    Ok(anchor) => PreparedGitContext {
                        anchor: Some(anchor),
                        warning: None,
                    },
                    Err(error) => PreparedGitContext {
                        anchor: None,
                        warning: Some(format!(
                            "Mesh imported the files, but could not prepare independent Git history: {error}"
                        )),
                    },
                },
            }
        }
        #[cfg(not(feature = "git-integration"))]
        {
            let _ = source;
            PreparedGitContext {
                warning: Some(
                    "This Mesh build does not include the optional Git integration.".to_owned(),
                ),
            }
        }
    }

    #[cfg(feature = "git-integration")]
    fn workspace_root_from_answer(answer: &str) -> Result<std::path::PathBuf, String> {
        Json::parse(answer)
            .map_err(|_| "The opened workspace returned an invalid result".to_owned())?
            .get("workspace")
            .and_then(|workspace| workspace.get("root"))
            .and_then(Json::as_text)
            .map(std::path::PathBuf::from)
            .ok_or_else(|| "The opened workspace returned no native root".to_owned())
    }

    fn workspace_binding_from_answer(answer: &str) -> Result<(String, String, String), String> {
        let parsed = Json::parse(answer)
            .map_err(|_| "The opened workspace returned an invalid result".to_owned())?;
        let workspace = parsed
            .get("workspace")
            .ok_or_else(|| "The opened workspace returned no workspace".to_owned())?;
        let field = |name: &str| {
            workspace
                .get(name)
                .and_then(Json::as_text)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("The opened workspace returned no {name}"))
        };
        Ok((field("root")?, field("digest")?, field("installation")?))
    }

    fn stable_storage_reference(workspace: &std::path::Path) -> Result<PathBuf, String> {
        let storage = workspace.parent().ok_or_else(|| {
            "The native workspace has no private storage parent for pinned setup".to_owned()
        })?;
        ProtectedWorkspaceRoot::inspect(storage)
            .and_then(ProtectedWorkspaceRoot::stable_reference)
            .map_err(|error| {
                format!("Mesh could not pin the native workspace's private storage: {error}")
            })
    }

    fn add_git_context_result(
        answer: &str,
        state: &str,
        warning: Option<String>,
    ) -> Result<String, String> {
        let parsed = Json::parse(answer)
            .map_err(|_| "The opened workspace returned an invalid result".to_owned())?;
        let Json::Object(mut fields) = parsed else {
            return Err("The opened workspace returned an invalid result".to_owned());
        };
        if fields
            .iter()
            .any(|(name, _)| name == "git_context" || name == "git_context_warning")
        {
            return Err("The opened workspace returned duplicate Git context state".to_owned());
        }
        fields.push(("git_context".to_owned(), Json::text(state)));
        fields.push((
            "git_context_warning".to_owned(),
            warning.map_or(Json::Null, Json::text),
        ));
        Ok(Json::Object(fields).encode())
    }

    fn install_prepared_git_context(
        source: &std::path::Path,
        answer: &str,
        prepared: PreparedGitContext,
        exact_worktree: bool,
        destination_identity: &GitSetupDestinationIdentity,
    ) -> Result<String, String> {
        let Some(warning) = prepared.warning else {
            #[cfg(feature = "git-integration")]
            {
                let Some(anchor) = prepared.anchor else {
                    return add_git_context_result(answer, "not-a-git-repository", None);
                };
                let destination = workspace_root_from_answer(answer)?;
                match fs::symlink_metadata(destination.join(".git")) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                        return match mesh_git_bridge::GitProvenanceAnchor::inspect(&destination) {
                            Ok(existing)
                                if existing.repository() == anchor.repository() =>
                            {
                                add_git_context_result(answer, "reused", None)
                            }
                            Ok(_) | Err(_) => add_git_context_result(
                                answer,
                                "unavailable",
                                Some("The native workspace already contains different Git metadata. Mesh preserved it and did not replace it.".to_owned()),
                            ),
                        }
                    }
                    Ok(_) => {
                        return add_git_context_result(
                            answer,
                            "unavailable",
                            Some("The native workspace's .git entry is not an independently owned directory. Mesh preserved it and did not follow or replace it.".to_owned()),
                        )
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return add_git_context_result(
                            answer,
                            "unavailable",
                            Some(format!("Mesh could not inspect the native workspace's Git metadata: {error}")),
                        )
                    }
                }
                let installed = if exact_worktree {
                    mesh_git_bridge::install_independent_git_context_for_destination(
                        source,
                        &destination,
                        *destination_identity,
                        &anchor,
                    )
                } else {
                    mesh_git_bridge::install_independent_git_history_for_destination(
                        source,
                        &destination,
                        *destination_identity,
                        &anchor,
                    )
                };
                return match installed {
                    Ok(_) => add_git_context_result(answer, "installed", None),
                    Err(mesh_git_bridge::GitContextError::DestinationMismatch) => Err(
                        "The native workspace changed before Mesh could install its independent Git context. Refresh before using this folder."
                            .to_owned(),
                    ),
                    Err(error) => add_git_context_result(
                        answer,
                        "unavailable",
                        Some(format!(
                            "Mesh preserved the native workspace, but could not install independent Git history: {error}"
                        )),
                    ),
                };
            }
            #[cfg(not(feature = "git-integration"))]
            {
                let _ = (source, exact_worktree, destination_identity);
                return add_git_context_result(answer, "unavailable", None);
            }
        };
        add_git_context_result(answer, "unavailable", Some(warning))
    }

    fn install_prepared_git_context_for_open_workspace(
        daemon: &LiveDaemon,
        source: &std::path::Path,
        answer: &str,
        prepared: PreparedGitContext,
        exact_worktree: bool,
    ) -> Result<String, String> {
        let (root, digest, installation) = workspace_binding_from_answer(answer)?;
        let destination_identity = git_setup_destination_identity(std::path::Path::new(&root))?;
        daemon
            .with_verified_managed_workspace(&root, &digest, &installation, || {
                #[cfg(test)]
                BEFORE_PREPARED_GIT_INSTALL.with(|hook| {
                    if let Some(hook) = hook.borrow_mut().take() {
                        hook();
                    }
                });
                install_prepared_git_context(
                    source,
                    answer,
                    prepared,
                    exact_worktree,
                    &destination_identity,
                )
                .map_err(mesh_daemon::ManagedTextFileError::Recovery)
            })
            .map_err(|error| error.to_string())
    }

    fn git_context_state_for_path(path: &std::path::Path) -> (&'static str, Option<String>) {
        #[cfg(feature = "git-integration")]
        {
            match fs::symlink_metadata(path.join(".git")) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => match mesh_git_bridge::GitProvenanceAnchor::inspect(path) {
                    Ok(_) => ("ready", None),
                    Err(error) => (
                        "unavailable",
                        Some(format!(
                            "The independent Git context is unavailable: {error}"
                        )),
                    ),
                },
                Ok(_) => (
                    "unavailable",
                    Some("The .git entry is not an independently owned directory, so Mesh did not follow it".to_owned()),
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => ("none", None),
                Err(error) => (
                    "unavailable",
                    Some(format!(
                        "The independent Git context is unavailable: {error}"
                    )),
                ),
            }
        }
        #[cfg(not(feature = "git-integration"))]
        {
            let _ = path;
            (
                "unavailable",
                Some("Git integration is not installed".to_owned()),
            )
        }
    }

    fn git_setup_destination_identity(
        path: &std::path::Path,
    ) -> Result<GitSetupDestinationIdentity, String> {
        #[cfg(feature = "git-integration")]
        {
            mesh_git_bridge::GitDestinationIdentity::inspect(path).map_err(|error| {
                format!("Mesh could not pin the native workspace before Git setup: {error}")
            })
        }
        #[cfg(not(feature = "git-integration"))]
        {
            let _ = path;
            Ok(())
        }
    }

    fn git_context_source_for_path(
        recent: &RecentWorkspace,
        path: &std::path::Path,
    ) -> Result<Option<std::path::PathBuf>, String> {
        #[cfg(feature = "git-integration")]
        {
            recent
                .load_entries()
                .map(|entries| {
                    entries
                    .into_iter()
                    .find(|entry| entry.path() == path)
                    .and_then(|entry| entry.project_root().map(std::path::Path::to_path_buf))
                })
                .map_err(|error| {
                    format!(
                        "Mesh could not read the original-folder association needed for Git setup: {error}"
                    )
                })
        }
        #[cfg(not(feature = "git-integration"))]
        {
            let _ = (recent, path);
            Ok(None)
        }
    }

    /// Upgrade an older app-managed import on first native use. The recent-workspace record is an
    /// app-owned navigation hint, not protocol authority: callers resolve it before acquiring the
    /// workspace-native lock, then publish only while exact shared authority is held. A current
    /// independent Git directory always wins so agent-local branches are never reset.
    fn ensure_git_context_for_source(
        path: &std::path::Path,
        source: &Result<Option<std::path::PathBuf>, String>,
        destination_identity: &GitSetupDestinationIdentity,
    ) -> (&'static str, Option<String>) {
        let current = git_context_state_for_path(path);
        if current.0 != "none" {
            return current;
        }
        #[cfg(feature = "git-integration")]
        {
            let source = match source {
                Ok(Some(source)) => source,
                Ok(None) => return current,
                Err(error) => return ("unavailable", Some(error.clone())),
            };
            let prepared = prepare_git_context(source);
            if let Some(warning) = prepared.warning {
                return ("unavailable", Some(warning));
            }
            let Some(anchor) = prepared.anchor else {
                return current;
            };
            match mesh_git_bridge::install_independent_git_history_for_destination(
                source,
                path,
                *destination_identity,
                &anchor,
            ) {
                Ok(_) => ("installed", None),
                Err(error) => (
                    "unavailable",
                    Some(format!(
                        "Mesh preserved the native workspace, but could not install independent Git history: {error}"
                    )),
                ),
            }
        }
        #[cfg(not(feature = "git-integration"))]
        {
            let _ = source;
            current
        }
    }

    fn ensure_git_context_for_unassigned_workspace(
        daemon: &LiveDaemon,
        recent: &RecentWorkspace,
        path: &std::path::Path,
        expected_workspace_root: &str,
        expected_workspace_digest: &str,
        expected_workspace_installation: &str,
    ) -> Result<(&'static str, Option<String>), String> {
        let source = git_context_source_for_path(recent, path);
        let destination_identity = git_setup_destination_identity(path)?;
        daemon
            .with_verified_managed_workspace(
                expected_workspace_root,
                expected_workspace_digest,
                expected_workspace_installation,
                || {
                    Ok(ensure_git_context_for_source(
                        path,
                        &source,
                        &destination_identity,
                    ))
                },
            )
            .map_err(|error| error.to_string())
    }

    fn with_agent_setup_authority<T>(
        daemon: &LiveDaemon,
        expected_workspace_root: &str,
        expected_workspace_digest: &str,
        expected_workspace_installation: &str,
        expected_agent_handoff_generation: &str,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _authority = daemon
            .lock_workspace_agent_setup(
                expected_workspace_root,
                expected_workspace_digest,
                expected_workspace_installation,
                expected_agent_handoff_generation,
            )
            .map_err(|error| error.to_string())?;
        operation()
    }

    #[cfg(feature = "git-integration")]
    fn approved_git_target(
        recent: &RecentWorkspace,
        managed_root: &std::path::Path,
    ) -> Result<(std::path::PathBuf, mesh_git_bridge::GitProvenanceAnchor), String> {
        let managed_root = fs::canonicalize(managed_root)
            .map_err(|error| format!("Mesh could not verify the native version folder: {error}"))?;
        let project_root = recent
            .load_entries()
            .map_err(|error| {
                format!("Mesh could not read the original-folder association: {error}")
            })?
            .into_iter()
            .find(|entry| entry.path() == managed_root)
            .and_then(|entry| entry.project_root().map(std::path::Path::to_path_buf))
            .ok_or_else(|| {
                "This Mesh workspace has no verified original Git project association".to_owned()
            })?;
        let managed =
            mesh_git_bridge::GitProvenanceAnchor::inspect(&managed_root).map_err(|error| {
                format!(
                    "Mesh could not verify the native version's independent Git history: {error}"
                )
            })?;
        let target = mesh_git_bridge::GitProvenanceAnchor::inspect(&project_root)
            .map_err(|error| format!("Mesh could not verify the original Git project: {error}"))?;
        if managed.repository() != target.repository() {
            return Err(
                "The native Mesh version and original project no longer share the same Git history"
                    .to_owned(),
            );
        }
        Ok((project_root, target))
    }

    struct WorkspaceVersionSource<'a> {
        root: &'a str,
        digest: &'a str,
        installation: &'a str,
        recent: &'a RecentWorkspace,
    }

    fn ensure_custom_version_destination_is_independent(
        recent: &RecentWorkspace,
        destination: &str,
    ) -> Result<Vec<ProtectedWorkspaceRoot>, String> {
        let destination = std::path::Path::new(destination);
        let destination = if destination.is_absolute() {
            destination.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|error| format!("Mesh could not resolve that version location: {error}"))?
                .join(destination)
        };
        let name = destination.file_name().ok_or_else(|| {
            "Choose a new folder outside every workspace already remembered by Mesh".to_owned()
        })?;
        let parent = destination
            .parent()
            .ok_or_else(|| {
                "Choose a new folder outside every workspace already remembered by Mesh".to_owned()
            })?
            .canonicalize()
            .map_err(|error| format!("Mesh could not verify that version location: {error}"))?;
        let destination = parent.join(name);
        let mut identities = std::collections::BTreeSet::new();
        for entry in recent.load_entries().map_err(|error| error.to_string())? {
            if entry.agent_handoff_installation().is_some() {
                let directory = entry.agent_handoff_directory().ok_or_else(|| {
                    "Finish the legacy agent assignment before choosing a custom saved-version location. No folder was created."
                        .to_owned()
                })?;
                identities.insert(
                    ProtectedWorkspaceRoot::from_directory_token(directory).map_err(|error| {
                        format!("Mesh could not verify an agent workspace assignment: {error}")
                    })?,
                );
            }
            let protected = std::iter::once(entry.path()).chain(entry.project_root());
            for protected in protected {
                let Ok(protected) = protected.canonicalize() else {
                    continue;
                };
                identities.insert(ProtectedWorkspaceRoot::inspect(&protected).map_err(
                    |error| format!("Mesh could not verify a remembered workspace: {error}"),
                )?);
                if destination == protected
                    || destination.starts_with(&protected)
                    || protected.starts_with(&destination)
                {
                    return Err(
                        "Choose a new folder outside every workspace and original project already remembered by Mesh. No folder was created."
                            .to_owned(),
                    );
                }
            }
        }
        Ok(identities.into_iter().collect())
    }

    fn open_workspace_version_copy(
        daemon: &LiveDaemon,
        version_workspaces: &VersionWorkspaceDirectory,
        operation: &str,
        destination: Option<&str>,
        origin_target: Option<&std::path::Path>,
        source: WorkspaceVersionSource<'_>,
        reuse_existing: bool,
    ) -> Result<String, String> {
        source
            .recent
            .ensure_can_remember_another()
            .map_err(|error| error.to_string())?;
        let git_context = prepare_git_context(std::path::Path::new(source.root));
        let managed_by_app = destination.is_none();
        let mut protected_roots = Vec::new();
        let app_managed;
        let destination = match destination {
            Some(path) if !path.trim().is_empty() => {
                protected_roots =
                    ensure_custom_version_destination_is_independent(source.recent, path)?;
                path
            }
            Some(_) => return Err(
                "A custom version location cannot be blank; clear it to let Mesh manage the copy"
                    .to_owned(),
            ),
            None => {
                if reuse_existing {
                    // A byte-exact checkout can still be in active use by an agent. Reusing that
                    // physical folder for ordinary navigation would put the stable human-facing
                    // path on the same writable directory, and "Switch and open in Codex" could
                    // hand it to a second agent without the explicit collision ceremony. Recent
                    // navigation is only a conservative handoff hint, so an unreadable record or
                    // an unresolvable candidate simply disables reuse; allocating a fresh copy is
                    // always the safe fallback.
                    let handed_off = source.recent.load_entries().ok();
                    for existing in version_workspaces
                        .reuse_candidates_for(operation)
                        .map_err(|error| error.to_string())?
                    {
                        let presented = mesh_daemon::workspace::presented_workspace_path(&existing)
                            .and_then(fs::canonicalize)
                            .ok();
                        let may_be_handed_off = match (&handed_off, presented.as_deref()) {
                            (Some(entries), Some(presented)) => entries.iter().any(|entry| {
                                entry.path() == presented
                                    && entry.agent_handoff_installation().is_some()
                            }),
                            _ => true,
                        };
                        if may_be_handed_off {
                            continue;
                        }
                        if let Some(answer) = daemon
                            .reopen_workspace_version_if_exact(
                                operation,
                                &existing,
                                source.root,
                                source.digest,
                                source.installation,
                                origin_target,
                            )
                            .map_err(|error| error.to_string())?
                        {
                            // A valid checkout may outlive a crash between the native open and the
                            // best-effort lookup-marker write. Exact daemon validation above is the
                            // authority; once it succeeds, restore the full-digest hint so later
                            // switches remain one click and do not allocate duplicate folders.
                            let _ = version_workspaces.record_source(&existing, operation);
                            return install_prepared_git_context_for_open_workspace(
                                daemon,
                                std::path::Path::new(source.root),
                                &answer.encode(),
                                git_context,
                                false,
                            );
                        }
                    }
                }
                app_managed = version_workspaces
                    .allocate(operation)
                    .map_err(|error| error.to_string())?;
                app_managed.to_str().ok_or_else(|| {
                    "The app-managed version location is not valid UTF-8".to_owned()
                })?
            }
        };
        let answer = daemon
            .fork_workspace_version_protected(
                WorkspaceVersionForkRequest::new(
                    operation,
                    destination,
                    source.root,
                    source.digest,
                    source.installation,
                    origin_target,
                )
                .protecting(&protected_roots),
            )
            .map_err(|error| error.to_string())?;
        if managed_by_app {
            if let Some(private_store) = answer
                .get("private_store")
                .and_then(Json::as_text)
                .map(std::path::Path::new)
            {
                // Reuse is only an optimization. The historical checkout is already open and
                // valid here, so a failed cache marker must not strand it behind an error.
                let _ = version_workspaces.record_source(private_store, operation);
            }
        }
        install_prepared_git_context_for_open_workspace(
            daemon,
            std::path::Path::new(source.root),
            &answer.encode(),
            git_context,
            false,
        )
    }

    /// Finish the native navigation side of a saved-version switch before the browser receives
    /// success. The daemon has already created and opened an independent checkout at this point;
    /// leaving the stable link and recent-workspace record to a later JavaScript call would make a
    /// stopped webview strand that valid checkout below private application data.
    fn commit_opened_workspace_version_navigation(
        daemon: &LiveDaemon,
        recent: &RecentWorkspace,
        active_workspace: &ActiveWorkspaceLink,
        answer: &str,
        export_root: Option<&std::path::Path>,
        inherited_project_root: Option<&std::path::Path>,
    ) -> Result<RecentWorkspaceStatus, String> {
        let answer = Json::parse(answer)
            .map_err(|_| "The opened workspace version returned an invalid result".to_owned())?;
        let workspace = answer
            .get("workspace")
            .ok_or_else(|| "The opened workspace version returned no workspace".to_owned())?;
        let expected_root = workspace
            .get("root")
            .and_then(Json::as_text)
            .ok_or_else(|| "The opened workspace version returned no root".to_owned())?;
        let expected_digest = workspace
            .get("digest")
            .and_then(Json::as_text)
            .ok_or_else(|| "The opened workspace version returned no digest".to_owned())?;
        let expected_installation = workspace
            .get("installation")
            .and_then(Json::as_text)
            .ok_or_else(|| "The opened workspace version returned no installation".to_owned())?;
        let source_point_ordinal = match answer.get("action").and_then(Json::as_text) {
            Some("workspace-version-opened-as-copy")
            | Some("workspace-version-reopened-existing-copy") => {
                let ordinal = answer
                    .get("source_ordinal")
                    .and_then(Json::as_u64)
                    .ok_or_else(|| {
                        "The opened workspace version returned no source point".to_owned()
                    })?;
                if ordinal == 0 {
                    return Err(
                        "The opened workspace version returned an invalid source point".to_owned(),
                    );
                }
                Some(ordinal)
            }
            _ => None,
        };
        let open = daemon
            .workspace_state()
            .map_err(|error| error.to_string())?;
        if open.root != expected_root
            || open.digest != expected_digest
            || open.installation != expected_installation
        {
            return Err("The opened workspace changed before its native navigation could be committed. Refresh before using either folder.".to_owned());
        }
        remember_current_workspace_navigation_with_project_root(
            daemon,
            recent,
            active_workspace,
            &open,
            export_root,
            inherited_project_root,
            source_point_ordinal,
        )
    }

    /// Return the exact navigation state committed by the native version switch in the same
    /// response as the opened workspace. The browser must not repeat the remember mutation merely
    /// to learn what this command already durably published.
    fn encode_opened_workspace_version_response(
        answer: &str,
        navigation: &RecentWorkspaceStatus,
    ) -> Result<String, String> {
        let parsed = Json::parse(answer)
            .map_err(|_| "The opened workspace version returned an invalid result".to_owned())?;
        let Json::Object(mut fields) = parsed else {
            return Err("The opened workspace version returned an invalid result".to_owned());
        };
        if fields.iter().any(|(name, _)| name == "navigation") {
            return Err(
                "The opened workspace version returned duplicate navigation state".to_owned(),
            );
        }
        let navigation = Json::parse(&navigation.to_json())
            .map_err(|_| "The committed workspace navigation could not be encoded".to_owned())?;
        fields.push(("navigation".to_owned(), navigation));
        Ok(Json::Object(fields).encode())
    }

    /// Refresh an already-installed app-owned Codex binding before a reused checkout becomes the
    /// stable native folder again.
    ///
    /// The linked private directory is writable by the same local account as the agent. Its exact
    /// one-file shape therefore proves only that no extra context appeared; it cannot prove that
    /// the expected `config.toml` bytes survived. Re-enter the native producer while the checkout
    /// is open but before navigation is published. Workspaces with an ordinary `.codex`
    /// directory remain user content and are not rewritten here.
    fn refresh_existing_codex_context(
        runtime: &DesktopRuntime,
        answer: &str,
    ) -> Result<(), String> {
        const PROJECT_LINK_TARGET: &str = "../integrations/codex";

        let parsed = Json::parse(answer)
            .map_err(|_| "The opened workspace version returned an invalid result".to_owned())?;
        let workspace = parsed
            .get("workspace")
            .ok_or_else(|| "The opened workspace version returned no workspace".to_owned())?;
        let root = workspace
            .get("root")
            .and_then(Json::as_text)
            .ok_or_else(|| "The opened workspace version returned no root".to_owned())?;
        let digest = workspace
            .get("digest")
            .and_then(Json::as_text)
            .ok_or_else(|| "The opened workspace version returned no digest".to_owned())?;
        let installation = workspace
            .get("installation")
            .and_then(Json::as_text)
            .ok_or_else(|| "The opened workspace version returned no installation".to_owned())?;
        let project_link = std::path::Path::new(root).join(".codex");
        let metadata = match fs::symlink_metadata(&project_link) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(format!(
                    "Mesh could not inspect the reused workspace's Codex context before opening it: {error}"
                ))
            }
        };
        if !metadata.file_type().is_symlink() {
            return Ok(());
        }
        let target = fs::read_link(&project_link).map_err(|error| {
            format!(
                "Mesh could not inspect the reused workspace's Codex context before opening it: {error}"
            )
        })?;
        if target != std::path::Path::new(PROJECT_LINK_TARGET) {
            return Err(
                "The reused workspace's Codex link changed before it could be opened. Mesh left the stable working folder unchanged."
                    .to_owned(),
            );
        }
        let executable = std::env::current_exe()
            .and_then(fs::canonicalize)
            .map_err(|error| format!("Mesh could not verify its installed executable: {error}"))?;
        let workspace_reference = ProtectedWorkspaceRoot::inspect(std::path::Path::new(root))
            .and_then(ProtectedWorkspaceRoot::stable_reference)
            .map_err(|error| {
                format!("Mesh could not pin the reused workspace's native folder: {error}")
            })?;
        let storage_reference = stable_storage_reference(std::path::Path::new(root))?;
        runtime
            .daemon
            .with_verified_managed_workspace(root, digest, installation, || {
                ensure_codex_project_config_at_references(
                    std::path::Path::new(root),
                    &workspace_reference,
                    &storage_reference,
                    &executable,
                    &runtime.endpoint,
                    installation,
                )
                .map_err(|error| mesh_daemon::ManagedTextFileError::Recovery(error.to_string()))
            })
            .map_err(|error| {
                format!(
                    "Mesh could not restore its exact Codex context before opening the reused workspace: {error}"
                )
            })?;
        Ok(())
    }

    fn finish_opened_workspace_version(
        runtime: &DesktopRuntime,
        answer: &str,
        export_root: Option<&std::path::Path>,
        inherited_project_root: Option<&std::path::Path>,
    ) -> Result<String, String> {
        refresh_existing_codex_context(runtime, answer)?;
        let status = commit_opened_workspace_version_navigation(
            &runtime.daemon,
            &runtime.recent,
            &runtime.active_workspace,
            answer,
            export_root,
            inherited_project_root,
        )?;
        let response = encode_opened_workspace_version_response(answer, &status)?;
        *runtime
            .recent_status
            .lock()
            .map_err(|_| "The recent workspace status is unavailable".to_owned())? = status;
        Ok(response)
    }

    // Version preview may traverse immutable history and CAS objects. Keep that bounded disk work
    // off the native application event thread just like the materialization commands below.
    #[tauri::command(async)]
    fn preview_managed_workspace_version(
        runtime: State<'_, DesktopRuntime>,
        operation: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        runtime
            .daemon
            .preview_workspace_version_for_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                &operation,
            )
            .map(|preview| preview.encode())
            .map_err(|error| error.to_string())
    }

    #[tauri::command(async)]
    fn open_managed_workspace_version(
        runtime: State<'_, DesktopRuntime>,
        operation: String,
        destination: Option<String>,
        export_root: Option<String>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let _ = ensure_git_context_for_unassigned_workspace(
            &runtime.daemon,
            &runtime.recent,
            std::path::Path::new(&expected_workspace_root),
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
        );
        let inherited_project_root = runtime.recent.load_entries().ok().and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.path().to_str() == Some(&expected_workspace_root))
                .and_then(|entry| entry.project_root().map(std::path::Path::to_path_buf))
        });
        let answer = open_workspace_version_copy(
            &runtime.daemon,
            &runtime.version_workspaces,
            &operation,
            destination.as_deref(),
            inherited_project_root.as_deref(),
            WorkspaceVersionSource {
                root: &expected_workspace_root,
                digest: &expected_workspace_digest,
                installation: &expected_workspace_installation,
                recent: &runtime.recent,
            },
            true,
        )?;
        finish_opened_workspace_version(
            &runtime,
            &answer,
            export_root.as_deref().map(std::path::Path::new),
            inherited_project_root.as_deref(),
        )
    }

    /// Allocate a new app-owned checkout for one additional long-running agent.
    ///
    /// This is deliberately a separate native capability from ordinary version navigation. An
    /// exact clean checkout may still be in use by another process, so this command has no custom
    /// destination and never consults the reusable-checkout cache.
    #[tauri::command(async)]
    fn open_fresh_agent_workspace(
        runtime: State<'_, DesktopRuntime>,
        operation: String,
        export_root: Option<String>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let _ = ensure_git_context_for_unassigned_workspace(
            &runtime.daemon,
            &runtime.recent,
            std::path::Path::new(&expected_workspace_root),
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
        );
        let inherited_project_root = runtime.recent.load_entries().ok().and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| entry.path().to_str() == Some(&expected_workspace_root))
                .and_then(|entry| entry.project_root().map(std::path::Path::to_path_buf))
        });
        let answer = open_workspace_version_copy(
            &runtime.daemon,
            &runtime.version_workspaces,
            &operation,
            None,
            inherited_project_root.as_deref(),
            WorkspaceVersionSource {
                root: &expected_workspace_root,
                digest: &expected_workspace_digest,
                installation: &expected_workspace_installation,
                recent: &runtime.recent,
            },
            false,
        )?;
        finish_opened_workspace_version(
            &runtime,
            &answer,
            export_root.as_deref().map(std::path::Path::new),
            inherited_project_root.as_deref(),
        )
    }

    /// Import a folder without asking the webview or the person to choose Mesh's private storage.
    ///
    /// A custom destination remains available for advanced use. The default is allocated only by
    /// the native host below its owner-only application-data directory. Import, workspace open,
    /// stable-link activation, recent-workspace persistence, and the ordinary-folder hint are
    /// committed before the browser receives success.
    #[tauri::command(async)]
    fn import_managed_workspace(
        runtime: State<'_, DesktopRuntime>,
        source: String,
        summary: String,
        destination: Option<String>,
    ) -> Result<String, String> {
        import_managed_workspace_for(runtime.inner(), source, summary, destination)
    }

    fn import_managed_workspace_for(
        runtime: &DesktopRuntime,
        source: String,
        summary: String,
        destination: Option<String>,
    ) -> Result<String, String> {
        runtime
            .recent
            .ensure_can_remember_another()
            .map_err(|error| error.to_string())?;
        let git_context = prepare_git_context(std::path::Path::new(&source));
        let app_managed;
        let mut protected_roots = Vec::new();
        let destination = match destination.as_deref() {
            Some(path) if !path.trim().is_empty() => {
                protected_roots =
                    ensure_custom_version_destination_is_independent(&runtime.recent, path)?;
                path
            }
            Some(_) => {
                return Err(
                    "A custom workspace location cannot be blank; clear it to let Mesh manage the workspace"
                        .to_owned(),
                )
            }
            None => {
                // A confirmed import can become durable through the live CLI before the desktop
                // publishes its recent-workspace record and stable navigation. Prefer that exact
                // currently open candidate before allocating another app-owned copy. The open
                // pathname is only a lookup hint: reopen_confirmed_folder_import_if_exact still
                // requires the canonical import receipt, the unchanged source snapshot, and all
                // private per-object origin receipts bound to this source directory identity.
                // Matching bytes at another path can therefore never adopt the live workspace.
                if let Ok(open) = runtime.daemon.workspace_state() {
                    let presented = std::path::Path::new(&open.root);
                    if presented
                        .file_name()
                        .is_some_and(mesh_daemon::workspace::is_presented_directory_name)
                    {
                        if let Some(candidate_store) = presented.parent() {
                            if let Some(answer) = runtime
                                .daemon
                                .reopen_confirmed_folder_import_if_exact(
                                    std::path::Path::new(&source),
                                    candidate_store,
                                    &summary,
                                )
                                .map_err(|error| error.to_string())?
                            {
                                let answer = install_prepared_git_context_for_open_workspace(
                                    &runtime.daemon,
                                    std::path::Path::new(&source),
                                    &answer.encode(),
                                    git_context,
                                    true,
                                )?;
                                return commit_managed_import_navigation(
                                    runtime,
                                    &source,
                                    &answer,
                                );
                            }
                        }
                    }
                }
                for candidate in runtime
                    .version_workspaces
                    .import_recovery_candidates(&summary)
                    .map_err(|error| error.to_string())?
                {
                    if let Some(answer) = runtime
                        .daemon
                        .reopen_confirmed_folder_import_if_exact(
                            std::path::Path::new(&source),
                            &candidate,
                            &summary,
                        )
                        .map_err(|error| error.to_string())?
                    {
                        let answer = install_prepared_git_context_for_open_workspace(
                            &runtime.daemon,
                            std::path::Path::new(&source),
                            &answer.encode(),
                            git_context,
                            true,
                        )?;
                        return commit_managed_import_navigation(
                            runtime,
                            &source,
                            &answer,
                        );
                    }
                }
                app_managed = runtime
                    .version_workspaces
                    .allocate_import(&summary)
                    .map_err(|error| error.to_string())?;
                app_managed.to_str().ok_or_else(|| {
                    "The app-managed workspace location is not valid UTF-8".to_owned()
                })?
            }
        };
        let mut parameters = vec![
            ("source", Json::text(&source)),
            ("destination", Json::text(destination)),
            ("summary", Json::text(&summary)),
        ];
        if !protected_roots.is_empty() {
            parameters.push((
                "protected_roots",
                Json::Array(
                    protected_roots
                        .into_iter()
                        .map(|root| Json::text(root.directory_token()))
                        .collect(),
                ),
            ));
        }
        let answer = call_daemon(
            &runtime.endpoint,
            "folder.import.confirm".to_owned(),
            Json::object(parameters).encode(),
        )?;
        let answer = install_prepared_git_context_for_open_workspace(
            &runtime.daemon,
            std::path::Path::new(&source),
            &answer,
            git_context,
            true,
        )?;
        commit_managed_import_navigation(runtime, &source, &answer)
    }

    fn commit_managed_import_navigation(
        runtime: &DesktopRuntime,
        source: &str,
        answer: &str,
    ) -> Result<String, String> {
        let status = commit_opened_workspace_version_navigation(
            &runtime.daemon,
            &runtime.recent,
            &runtime.active_workspace,
            answer,
            Some(std::path::Path::new(&source)),
            Some(std::path::Path::new(&source)),
        )?;
        let response = encode_opened_workspace_version_response(answer, &status)?;
        *runtime
            .recent_status
            .lock()
            .map_err(|_| "The recent workspace status is unavailable".to_owned())? = status;
        Ok(response)
    }

    #[tauri::command(async)]
    fn rollback_managed_workspace(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                runtime
                    .daemon
                    .rollback_folder_import_for_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                    )
                    .map(|answer| answer.encode())
                    .map_err(|error| daemon_refusal(&error.code, &error.message))
            },
        )
    }

    // The IPC proxy can wait for the local daemon's bounded socket timeout. Running it on the
    // async command executor prevents every ordinary state refresh from blocking AppKit.
    #[tauri::command(async)]
    fn daemon_call(
        runtime: State<'_, DesktopRuntime>,
        method: String,
        params_json: String,
    ) -> Result<String, String> {
        // The daemon's general `workspace.open` method deliberately creates an empty workspace
        // for CLI callers. In the desktop, however, this method is reached only by the control
        // labelled "Open existing managed workspace". Forwarding it unchanged could initialize
        // `.mesh` inside an ordinary project selected in the wrong form and then describe that
        // empty history as reopened. Keep the UI's promise exact: only an already-initialized
        // journal may pass this bridge, while first-time folders continue through verified Import.
        if method == "workspace.open" {
            return reopen_existing_managed_workspace(&runtime.daemon, &params_json);
        }
        // The generic browser bridge is intentionally read-only. Every desktop mutation has a
        // dedicated native command that binds its exact workspace authority and, where needed,
        // participates in the cross-process recent-workspace transaction. Forwarding a mutation
        // here would let an older renderer bypass those checks (notably import rollback).
        if !desktop_daemon_call_is_read_only(&method) {
            return Err(format!(
                "Desktop daemon method {method} requires a dedicated verified native command"
            ));
        }
        call_daemon(&runtime.endpoint, method, params_json)
    }

    fn desktop_daemon_call_is_read_only(method: &str) -> bool {
        matches!(method, "workspace.state" | "folder.import.preview")
    }

    fn reopen_existing_managed_workspace(
        daemon: &LiveDaemon,
        params_json: &str,
    ) -> Result<String, String> {
        let params = Json::parse(params_json).map_err(|error| error.to_string())?;
        let Json::Object(fields) = params else {
            return Err("Desktop calls require one JSON object".to_owned());
        };
        if fields.len() != 1 || fields[0].0 != "path" {
            return Err(
                "Opening an existing managed workspace requires exactly one path".to_owned(),
            );
        }
        let path = fields[0]
            .1
            .as_text()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| "The managed workspace path must be a non-empty string".to_owned())?;
        daemon
            .reopen_existing_workspace(std::path::Path::new(path))
            .map(|workspace| workspace.to_json().encode())
            .map_err(|error| {
                let message = match error.code.as_str() {
                    "workspace-unreachable" => "Mesh could not reach an initialized saved workspace in this folder. Choose Import if it is an ordinary project folder; nothing was opened, initialized, or changed.",
                    "workspace-payload-store-unreachable" => "Mesh found the saved workspace, but its private file history is unavailable. Leave the folder unchanged and try again; nothing was opened or changed.",
                    "workspace-index-unavailable" => "Mesh found the saved workspace, but its private history index is unavailable. Leave the folder unchanged and try again; nothing was opened or changed.",
                    "workspace-damaged" | "workspace-nothing-readable" | "workspace-contradictory" => "Mesh found the saved workspace, but could not verify its private history. Leave the folder unchanged and copy diagnostics before repairing or removing anything; nothing was opened or changed.",
                    _ => "Mesh could not verify this saved workspace. Nothing was opened, initialized, or changed.",
                };
                daemon_refusal(
                    &error.code,
                    message,
                )
            })
    }

    fn call_daemon(
        endpoint: &std::path::Path,
        method: String,
        params_json: String,
    ) -> Result<String, String> {
        let reply_timeout = daemon_reply_timeout(&method);
        call_daemon_with_timeout(endpoint, method, params_json, reply_timeout)
    }

    fn daemon_reply_timeout(method: &str) -> Duration {
        if method == "folder.import.confirm" {
            DAEMON_IMPORT_REPLY_TIMEOUT
        } else if matches!(method, "workspace.open" | "workspace.state") {
            DAEMON_WORKSPACE_READ_REPLY_TIMEOUT
        } else {
            DAEMON_REPLY_TIMEOUT
        }
    }

    fn call_daemon_with_timeout(
        endpoint: &std::path::Path,
        method: String,
        params_json: String,
        reply_timeout: Duration,
    ) -> Result<String, String> {
        let params = Json::parse(&params_json).map_err(|error| error.to_string())?;
        if !params.is_object() {
            return Err("Desktop calls require one JSON object".to_owned());
        }
        let mut stream = UnixStream::connect(endpoint)
            .map_err(|error| format!("The local service is unavailable: {error}"))?;
        stream
            .set_read_timeout(Some(reply_timeout))
            .map_err(|error| format!("The local service reply deadline is unavailable: {error}"))?;
        stream
            .set_write_timeout(Some(reply_timeout))
            .map_err(|error| format!("The local service send deadline is unavailable: {error}"))?;
        let hello = ClientMessage::Hello {
            id: 1,
            versions: vec![SURFACE_VERSION],
            session: DESKTOP_SESSION.to_owned(),
        };
        writeln!(stream, "{}", hello.encode()).map_err(|error| error.to_string())?;
        stream.flush().map_err(|error| error.to_string())?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
        let mut chunks = ChunkAssembler::default();
        let welcome = read_message(&mut reader, &mut chunks)?;
        match welcome {
            DaemonMessage::Welcome {
                id: 1,
                version,
                session,
                surface_version,
                ..
            } if version == SURFACE_VERSION
                && session == DESKTOP_SESSION
                && surface_version >= version => {}
            DaemonMessage::Refused {
                id: 1,
                code,
                message,
                ..
            } => return Err(daemon_refusal(&code, &message)),
            other => {
                return Err(format!(
                    "The local service returned an unexpected {PROTOCOL} greeting: {other:?}"
                ))
            }
        }
        let call = ClientMessage::Call {
            id: 2,
            method,
            version: SURFACE_VERSION,
            params,
        };
        writeln!(stream, "{}", call.encode()).map_err(|error| error.to_string())?;
        stream.flush().map_err(|error| error.to_string())?;
        match read_message(&mut reader, &mut chunks)? {
            DaemonMessage::Result { id: 2, value } => Ok(value.encode()),
            DaemonMessage::Failed {
                id: 2,
                code,
                message,
            } => Err(daemon_refusal(&code, &message)),
            other => Err(format!(
                "The local service returned an unexpected answer: {other:?}"
            )),
        }
    }

    // Native discovery and mutation can traverse or hash a complete working tree. Every direct
    // filesystem command uses Tauri's async dispatch context so the visible five-second scan and
    // explicit save loop never occupy the application event thread.
    #[tauri::command(async)]
    fn native_capture_preference(runtime: State<'_, DesktopRuntime>) -> Result<String, String> {
        let enabled = runtime
            .native_capture
            .enabled()
            .map_err(|error| error.to_string())?;
        Ok(Json::object([("enabled", Json::Bool(enabled))]).encode())
    }

    #[tauri::command(async)]
    fn set_native_capture_preference(
        runtime: State<'_, DesktopRuntime>,
        enabled: bool,
    ) -> Result<String, String> {
        runtime
            .native_capture
            .set_enabled(enabled)
            .map_err(|error| error.to_string())?;
        Ok(Json::object([("enabled", Json::Bool(enabled))]).encode())
    }

    #[tauri::command]
    async fn inspect_agent_finish_preflight(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        expected_agent_handoff_generation: String,
    ) -> Result<String, String> {
        parse_agent_handoff_generation(&expected_agent_handoff_generation)?;
        let _ = runtime
            .renderer_proof
            .report_checkpoint("agent-handoff-preflight-command-entered");
        let daemon = Arc::clone(&runtime.daemon);
        let preflight = tauri::async_runtime::spawn_blocking(move || {
            daemon.inspect_agent_finish_preflight(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                &expected_agent_handoff_generation,
            )
        })
        .await
        .map_err(|_| "The agent-finish inspection stopped unexpectedly".to_owned())?
        .map_err(|error| error.to_string())?;
        runtime.renderer_proof.record_agent_handoff_preflight(
            preflight.root(),
            preflight.installation(),
            preflight.generation(),
        )?;
        let _ = runtime
            .renderer_proof
            .report_checkpoint("agent-handoff-preflight-command-returned");
        Ok(Json::object([
            ("schema", Json::text("mesh.agent-finish-preflight/v1")),
            ("workspace_root", Json::text(preflight.root())),
            ("workspace_digest", Json::text(preflight.digest())),
            (
                "workspace_installation",
                Json::text(preflight.installation()),
            ),
            (
                "agent_handoff_generation",
                Json::text(preflight.generation()),
            ),
            (
                "managed_files",
                Json::Array(
                    preflight
                        .managed_files()
                        .iter()
                        .map(|file| {
                            Json::object([
                                ("path", Json::text(file.path())),
                                ("current_version", Json::text(file.current_version())),
                                ("byte_count", Json::Number(file.byte_count())),
                                (
                                    "content_digest",
                                    Json::text(file.content_digest().to_string()),
                                ),
                                ("executable", Json::Bool(file.executable())),
                                (
                                    "modified_from_current_version",
                                    Json::Bool(file.modified_from_current_version()),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "native_files",
                Json::Array(
                    preflight
                        .native_files()
                        .iter()
                        .map(|file| {
                            Json::object([
                                ("path", Json::text(file.path())),
                                ("byte_count", Json::Number(file.byte_count())),
                                (
                                    "content_digest",
                                    Json::text(file.content_digest().to_string()),
                                ),
                                ("executable", Json::Bool(file.executable())),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "native_directories",
                Json::Array(
                    preflight
                        .native_directories()
                        .iter()
                        .map(|directory| {
                            Json::object([
                                ("path", Json::text(directory.path())),
                                ("installation", Json::text(directory.installation())),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "missing_files",
                Json::Array(
                    preflight
                        .missing_files()
                        .iter()
                        .map(|file| {
                            Json::object([
                                ("path", Json::text(file.path())),
                                ("current_version", Json::text(file.current_version())),
                                (
                                    "content_digest",
                                    Json::text(file.content_digest().to_string()),
                                ),
                                ("executable", Json::Bool(file.executable())),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "unsupported_entries",
                Json::Array(
                    preflight
                        .unsupported_entries()
                        .iter()
                        .map(|entry| {
                            Json::object([
                                ("path", Json::text(entry.path())),
                                ("kind", Json::text(entry.kind())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
        .encode())
    }

    #[tauri::command]
    async fn inspect_agent_live_work(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        expected_agent_handoff_generation: String,
    ) -> Result<String, String> {
        parse_agent_handoff_generation(&expected_agent_handoff_generation)?;
        let daemon = Arc::clone(&runtime.daemon);
        let inspection = tauri::async_runtime::spawn_blocking(move || {
            daemon.inspect_agent_finish_preflight(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                &expected_agent_handoff_generation,
            )
        })
        .await
        .map_err(|_| "The live agent inspection stopped unexpectedly".to_owned())?
        .map_err(|error| error.to_string())?;
        let mut changes = inspection
            .managed_files()
            .iter()
            .filter(|file| file.modified_from_current_version())
            .map(|file| {
                Json::object([
                    ("path", Json::text(file.path())),
                    ("kind", Json::text("modified-file")),
                ])
            })
            .collect::<Vec<_>>();
        changes.extend(inspection.native_files().iter().map(|file| {
            Json::object([
                ("path", Json::text(file.path())),
                ("kind", Json::text("new-file")),
            ])
        }));
        changes.extend(inspection.native_directories().iter().map(|directory| {
            Json::object([
                ("path", Json::text(directory.path())),
                ("kind", Json::text("new-folder")),
            ])
        }));
        changes.extend(inspection.missing_files().iter().map(|file| {
            Json::object([
                ("path", Json::text(file.path())),
                ("kind", Json::text("missing-file")),
            ])
        }));
        changes.extend(inspection.unsupported_entries().iter().map(|entry| {
            Json::object([
                ("path", Json::text(entry.path())),
                ("kind", Json::text("unsupported")),
            ])
        }));
        Ok(Json::object([
            ("schema", Json::text("mesh.agent-live-work/v1")),
            ("workspace_root", Json::text(inspection.root())),
            ("workspace_digest", Json::text(inspection.digest())),
            (
                "workspace_installation",
                Json::text(inspection.installation()),
            ),
            (
                "agent_handoff_generation",
                Json::text(inspection.generation()),
            ),
            ("changes", Json::Array(changes)),
        ])
        .encode())
    }

    #[tauri::command]
    async fn inspect_agent_live_file(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
        expected_agent_handoff_generation: String,
        relative_path: String,
    ) -> Result<String, String> {
        parse_agent_handoff_generation(&expected_agent_handoff_generation)?;
        let snapshot = runtime
            .daemon
            .inspect_agent_live_file(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                &expected_agent_handoff_generation,
                &relative_path,
            )
            .map_err(|error| error.to_string())?;
        let mut preview_kind = "metadata";
        let mut image_data_url = None;
        let mut preview_error = None;
        if snapshot.text().is_some() {
            preview_kind = "text";
        } else if let Some(image) = live_raster_data_url(snapshot.path(), snapshot.bytes()) {
            preview_kind = "image";
            image_data_url = Some(image);
        } else if !snapshot.bytes().is_empty()
            && inspection_extension(snapshot.path(), snapshot.bytes()).is_some()
        {
            let path = snapshot.path().to_owned();
            let bytes = snapshot.bytes().to_vec();
            let page = path.to_ascii_lowercase().ends_with(".pdf").then_some(1);
            match tauri::async_runtime::spawn_blocking(move || render_artifact(&path, &bytes, page))
                .await
                .map_err(|_| "The live artifact preview stopped unexpectedly".to_owned())?
            {
                Ok(rendered) => {
                    preview_kind = "artifact";
                    image_data_url = Some(format!(
                        "data:image/png;base64,{}",
                        encode_base64(&rendered.png)
                    ));
                }
                Err(error) => preview_error = Some(error.to_string()),
            }
        }
        Ok(Json::object([
            ("schema", Json::text("mesh.agent-live-file/v1")),
            ("workspace_root", Json::text(snapshot.root())),
            ("workspace_digest", Json::text(snapshot.digest())),
            (
                "workspace_installation",
                Json::text(snapshot.installation()),
            ),
            (
                "agent_handoff_generation",
                Json::text(snapshot.generation()),
            ),
            ("path", Json::text(snapshot.path())),
            ("kind", Json::text(snapshot.kind())),
            ("byte_count", Json::Number(snapshot.byte_count())),
            ("content_digest", Json::text(snapshot.content_digest())),
            ("executable", Json::Bool(snapshot.executable())),
            ("text", snapshot.text().map_or(Json::Null, Json::text)),
            ("preview_kind", Json::text(preview_kind)),
            (
                "image_data_url",
                image_data_url.map_or(Json::Null, Json::text),
            ),
            (
                "preview_error",
                preview_error.map_or(Json::Null, Json::text),
            ),
            ("mutable", Json::Bool(true)),
            ("recorded", Json::Bool(false)),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn inspect_managed_file(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let (file, baseline_text) = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || {
                    runtime.daemon.inspect_managed_file_with_durable_text(
                        &relative_path,
                        MAX_WORKSPACE_EDITOR_TEXT_BYTES,
                    )
                },
            )
            .map_err(|error| error.to_string())?;
        let editor_text = (file.byte_count()
            <= u64::try_from(MAX_WORKSPACE_EDITOR_TEXT_BYTES).unwrap_or(u64::MAX))
        .then(|| file.text())
        .flatten();
        Ok(Json::object([
            ("path", Json::text(file.path())),
            ("current_version", Json::text(file.current_version())),
            ("byte_count", Json::Number(file.byte_count())),
            (
                "content_digest",
                Json::text(file.content_digest().to_string()),
            ),
            ("executable", Json::Bool(file.executable())),
            ("text", editor_text.map_or(Json::Null, Json::text)),
            (
                "baseline_text",
                baseline_text.as_deref().map_or(Json::Null, Json::text),
            ),
            ("text_editable", Json::Bool(editor_text.is_some())),
            (
                "modified_from_current_version",
                Json::Bool(file.modified_from_current_version()),
            ),
            (
                "max_text_bytes",
                Json::Number(MAX_WORKSPACE_EDITOR_TEXT_BYTES as u64),
            ),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn inspect_native_file(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let file = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || runtime.daemon.inspect_native_untracked_file(&relative_path),
            )
            .map_err(|error| error.to_string())?;
        let preview_text = (file.byte_count()
            <= u64::try_from(MAX_WORKSPACE_EDITOR_TEXT_BYTES).unwrap_or(u64::MAX))
        .then(|| file.text())
        .flatten();
        Ok(Json::object([
            ("path", Json::text(file.path())),
            ("byte_count", Json::Number(file.byte_count())),
            (
                "content_digest",
                Json::text(file.content_digest().to_string()),
            ),
            ("executable", Json::Bool(file.executable())),
            ("text", preview_text.map_or(Json::Null, Json::text)),
            ("text_editable", Json::Bool(false)),
            ("native_untracked", Json::Bool(true)),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn inspect_managed_directory_installation(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let directory = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || {
                    runtime
                        .daemon
                        .inspect_managed_directory_installation(&relative_path)
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(Json::object([
            ("path", Json::text(directory.path())),
            ("installation", Json::text(directory.installation())),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn discover_native_missing_files(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let missing = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || runtime.daemon.native_missing_files(),
            )
            .map_err(|error| error.to_string())?;
        Ok(Json::Array(
            missing
                .into_iter()
                .map(|file| {
                    Json::object([
                        ("path", Json::text(file.path())),
                        ("current_version", Json::text(file.current_version())),
                        (
                            "content_digest",
                            Json::text(file.content_digest().to_string()),
                        ),
                        ("executable", Json::Bool(file.executable())),
                    ])
                })
                .collect(),
        )
        .encode())
    }

    #[tauri::command(async)]
    fn discover_native_directories(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let directories = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || runtime.daemon.native_untracked_directories(),
            )
            .map_err(|error| error.to_string())?;
        Ok(Json::Array(
            directories
                .into_iter()
                .map(|directory| {
                    Json::object([
                        ("path", Json::text(directory.path())),
                        ("installation", Json::text(directory.installation())),
                    ])
                })
                .collect(),
        )
        .encode())
    }

    #[tauri::command(async)]
    // Tauri exposes these as explicit named wire fields; folding either exact-state binding into
    // an opaque string would make the mutation contract less reviewable and less fail-closed.
    #[allow(clippy::too_many_arguments)]
    fn preserve_managed_text(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        text: String,
        expected_content_digest: String,
        expected_executable: bool,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let expected_content_digest = ManagedContentDigest::parse_hex(&expected_content_digest)
            .map_err(|_| "The inspected managed-file digest is invalid".to_owned())?;
        let saved = with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.preserve_managed_text_edit(
                                &relative_path,
                                &text,
                                expected_content_digest,
                                expected_executable,
                            )
                        },
                    )
                    .map_err(|error| error.to_string())
            },
        )?;
        Ok(Json::object([
            ("path", Json::text(saved.path())),
            ("recovery", Json::text(saved.recovery().to_string())),
            (
                "content_digest",
                Json::text(saved.content_digest().to_string()),
            ),
            ("stable_after_idle", Json::Bool(saved.stable_after_idle())),
            ("recovery_preserved", Json::Bool(true)),
            ("saved_privately", Json::Bool(false)),
            ("state", Json::text("working")),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn save_managed_private(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        expected_content_digest: String,
        expected_executable: bool,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let expected_content_digest = ManagedContentDigest::parse_hex(&expected_content_digest)
            .map_err(|_| "The inspected managed-file digest is invalid".to_owned())?;
        let saved = with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let author = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local author key is unavailable".to_owned())?;
                let public_key = author.public_key().public_key();
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.save_managed_file_privately(
                                &relative_path,
                                expected_content_digest,
                                expected_executable,
                                public_key,
                                |payload| author.sign(payload),
                            )
                        },
                    )
                    .map_err(|error| error.to_string())
            },
        )?;
        Ok(Json::object([
            ("path", Json::text(saved.path())),
            ("version", Json::text(saved.version())),
            ("manifest", Json::text(saved.manifest())),
            ("changeset", Json::text(saved.changeset())),
            ("stable_after_idle", Json::Bool(saved.stable_after_idle())),
            ("saved_privately", Json::Bool(saved.meaningful_saved())),
            (
                "author_authenticated",
                Json::Bool(saved.author_authenticated()),
            ),
            (
                "state",
                Json::text(if saved.meaningful_saved() {
                    "saved_privately"
                } else {
                    "working"
                }),
            ),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn adopt_native_file(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        expected_content_digest: String,
        expected_executable: bool,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let expected_content_digest = ManagedContentDigest::parse_hex(&expected_content_digest)
            .map_err(|_| "The inspected native-file digest is invalid".to_owned())?;
        let saved = with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let author = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local author key is unavailable".to_owned())?;
                let public_key = author.public_key().public_key();
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.adopt_native_file_privately(
                                &relative_path,
                                expected_content_digest,
                                expected_executable,
                                public_key,
                                |payload| author.sign(payload),
                            )
                        },
                    )
                    .map_err(|error| error.to_string())
            },
        )?;
        Ok(Json::object([
            ("path", Json::text(saved.path())),
            ("version", Json::text(saved.version())),
            ("manifest", Json::text(saved.manifest())),
            ("changeset", Json::text(saved.changeset())),
            ("stable_after_idle", Json::Bool(saved.stable_after_idle())),
            ("saved_privately", Json::Bool(saved.meaningful_saved())),
            (
                "author_authenticated",
                Json::Bool(saved.author_authenticated()),
            ),
            (
                "state",
                Json::text(if saved.meaningful_saved() {
                    "saved_privately"
                } else {
                    "working"
                }),
            ),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn adopt_native_directory(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        expected_directory_installation: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let author = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local author key is unavailable".to_owned())?;
                let public_key = author.public_key().public_key();
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.adopt_native_directory_privately(
                                &relative_path,
                                &expected_directory_installation,
                                public_key,
                                |payload| author.sign(payload),
                            )
                        },
                    )
                    .map(|change| managed_entry_change_json(&change))
                    .map_err(|error| error.to_string())
            },
        )
    }

    #[tauri::command(async)]
    fn discover_retired_exports(
        runtime: State<'_, DesktopRuntime>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let entries = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || runtime.daemon.retired_export_entries(),
            )
            .map_err(|error| error.to_string())?;
        Ok(Json::Array(
            entries
                .iter()
                .map(|entry| {
                    Json::object([
                        ("path", Json::text(entry.path())),
                        ("type", Json::text(entry.entry_type())),
                    ])
                })
                .collect(),
        )
        .encode())
    }

    #[tauri::command(async)]
    fn preview_retired_export(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        target_root: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let preview = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || {
                    runtime
                        .daemon
                        .preview_retired_export(&relative_path, std::path::Path::new(&target_root))
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(Json::object([
            ("path", Json::text(preview.path())),
            ("type", Json::text(preview.entry_type())),
            (
                "source_version",
                preview.source_version().map_or(Json::Null, Json::text),
            ),
            (
                "source_content_digest",
                preview
                    .source_content_digest()
                    .map_or(Json::Null, |digest| Json::text(digest.to_string())),
            ),
            (
                "source_executable",
                preview.source_executable().map_or(Json::Null, Json::Bool),
            ),
            ("target_root", Json::text(preview.target_root())),
            (
                "target_installation",
                Json::text(preview.target_installation()),
            ),
            (
                "target_parent_installation",
                Json::text(preview.target_parent_installation()),
            ),
            (
                "target_entry_installation",
                preview
                    .target_entry_installation()
                    .map_or(Json::Null, Json::text),
            ),
            (
                "target_content_digest",
                preview
                    .target_content_digest()
                    .map_or(Json::Null, |digest| Json::text(digest.to_string())),
            ),
            (
                "target_executable",
                preview.target_executable().map_or(Json::Null, Json::Bool),
            ),
            ("removable", Json::Bool(preview.removable())),
            ("status", Json::text(preview.status())),
        ])
        .encode())
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    fn remove_retired_export(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        target_root: String,
        expected_entry_type: String,
        expected_source_version: Option<String>,
        expected_source_digest: Option<String>,
        expected_source_executable: Option<bool>,
        expected_target_installation: String,
        expected_target_parent_installation: String,
        expected_target_entry_installation: Option<String>,
        expected_target_digest: Option<String>,
        expected_target_executable: Option<bool>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let source_digest = expected_source_digest
            .as_deref()
            .map(ManagedContentDigest::parse_hex)
            .transpose()
            .map_err(|_| "The prior saved export digest is invalid".to_owned())?;
        let target_digest = expected_target_digest
            .as_deref()
            .map(ManagedContentDigest::parse_hex)
            .transpose()
            .map_err(|_| "The stale destination digest is invalid".to_owned())?;
        let removed = with_verified_export_workspace(
            &runtime,
            &target_root,
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
            |targets_original| {
                let remove = if targets_original {
                    LiveDaemon::remove_shared_retired_export
                } else {
                    LiveDaemon::remove_retired_export
                };
                remove(
                    &runtime.daemon,
                    &relative_path,
                    std::path::Path::new(&target_root),
                    &expected_entry_type,
                    expected_source_version.as_deref(),
                    source_digest,
                    expected_source_executable,
                    &expected_target_installation,
                    &expected_target_parent_installation,
                    expected_target_entry_installation.as_deref(),
                    target_digest,
                    expected_target_executable,
                )
            },
        )?;
        Ok(Json::object([
            ("path", Json::text(removed.path())),
            ("type", Json::text(removed.entry_type())),
            ("target_root", Json::text(removed.target_root())),
            ("removed", Json::Bool(true)),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn preview_managed_exports(
        runtime: State<'_, DesktopRuntime>,
        target_root: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let previews = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || {
                    runtime
                        .daemon
                        .preview_all_managed_file_exports(std::path::Path::new(&target_root))
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(Json::Array(
            previews
                .iter()
                .map(|preview| managed_file_export_preview_json(preview, false))
                .collect(),
        )
        .encode())
    }

    fn managed_file_export_preview_json(
        preview: &ManagedFileExportPreview,
        include_text: bool,
    ) -> Json {
        Json::object([
            ("path", Json::text(preview.path())),
            ("source_version", Json::text(preview.source_version())),
            (
                "source_byte_count",
                Json::Number(preview.source_byte_count()),
            ),
            (
                "source_content_digest",
                Json::text(preview.source_content_digest().to_string()),
            ),
            ("source_executable", Json::Bool(preview.source_executable())),
            (
                "source_text",
                if include_text {
                    preview.source_text().map_or(Json::Null, Json::text)
                } else {
                    Json::Null
                },
            ),
            ("target_root", Json::text(preview.target_root())),
            (
                "target_installation",
                Json::text(preview.target_installation()),
            ),
            (
                "target_parent_installation",
                Json::text(preview.target_parent_installation()),
            ),
            (
                "target_file_installation",
                preview
                    .target_file_installation()
                    .map_or(Json::Null, Json::text),
            ),
            ("target_exists", Json::Bool(preview.target_exists())),
            (
                "target_byte_count",
                preview.target_byte_count().map_or(Json::Null, Json::Number),
            ),
            (
                "target_content_digest",
                preview
                    .target_content_digest()
                    .map_or(Json::Null, |digest| Json::text(digest.to_string())),
            ),
            (
                "target_executable",
                preview.target_executable().map_or(Json::Null, Json::Bool),
            ),
            (
                "target_text",
                if include_text {
                    preview.target_text().map_or(Json::Null, Json::text)
                } else {
                    Json::Null
                },
            ),
            ("identical", Json::Bool(preview.identical())),
            ("target_relation", Json::text(preview.target_relation())),
            ("replace_allowed", Json::Bool(preview.replace_allowed())),
        ])
    }

    #[tauri::command(async)]
    fn preview_managed_export(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        target_root: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let preview = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || {
                    runtime.daemon.preview_managed_file_export(
                        &relative_path,
                        std::path::Path::new(&target_root),
                    )
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(managed_file_export_preview_json(&preview, true).encode())
    }

    #[tauri::command(async)]
    fn preview_managed_directory_export(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        target_root: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let preview = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || {
                    runtime.daemon.preview_managed_directory_export(
                        &relative_path,
                        std::path::Path::new(&target_root),
                    )
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(Json::object([
            ("path", Json::text(preview.path())),
            (
                "source_directory_installation",
                Json::text(preview.source_directory_installation()),
            ),
            ("target_root", Json::text(preview.target_root())),
            (
                "target_installation",
                Json::text(preview.target_installation()),
            ),
            (
                "target_parent_installation",
                Json::text(preview.target_parent_installation()),
            ),
            (
                "target_directory_installation",
                preview
                    .target_directory_installation()
                    .map_or(Json::Null, Json::text),
            ),
            ("target_exists", Json::Bool(preview.target_exists())),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn preview_managed_directory_exports(
        runtime: State<'_, DesktopRuntime>,
        target_root: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let preview = runtime
            .daemon
            .with_verified_managed_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                || {
                    runtime
                        .daemon
                        .preview_managed_directory_exports(std::path::Path::new(&target_root))
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(Json::object([
            ("target_root", Json::text(preview.target_root())),
            (
                "target_installation",
                Json::text(preview.target_installation()),
            ),
            (
                "missing_paths",
                Json::Array(preview.missing_paths().iter().map(Json::text).collect()),
            ),
        ])
        .encode())
    }

    fn managed_directory_export_json(exported: &ManagedDirectoryExport) -> Json {
        Json::object([
            ("path", Json::text(exported.path())),
            ("target_root", Json::text(exported.target_root())),
            ("installation", Json::text(exported.installation())),
            ("created", Json::Bool(exported.created())),
        ])
    }

    fn export_targets_original_project(
        recent: &RecentWorkspace,
        expected_workspace_root: &str,
        target_root: &str,
    ) -> Result<bool, String> {
        let entries = recent.load_entries().map_err(|error| {
            format!(
                "Mesh could not verify the original-project association before exporting: {error}"
            )
        })?;
        let workspace = fs::canonicalize(expected_workspace_root).map_err(|error| {
            format!("Mesh could not verify the managed workspace before exporting: {error}")
        })?;
        let entry = entries
            .iter()
            .find(|entry| entry.path() == workspace)
            .ok_or_else(|| {
                "Mesh could not find the verified workspace in its navigation record. Refresh or reopen it before exporting."
                    .to_owned()
            })?;
        let target = fs::canonicalize(target_root).map_err(|error| {
            format!("Mesh could not verify the selected export folder: {error}")
        })?;
        let project = entry
            .project_root()
            .map(fs::canonicalize)
            .transpose()
            .map_err(|error| {
                format!("Mesh could not verify the remembered original project folder: {error}")
            })?;
        // Without a durable native-host association, this target might be an original folder
        // whose navigation hint was lost or deliberately forgotten. Preserve explicit export
        // after approval, but still reject overlap with every known protected tree below.
        let mut targets_original = project.is_none();
        for remembered in &entries {
            let protected_workspace = match fs::canonicalize(remembered.path()) {
                Ok(path) => path,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(format!(
                        "Mesh could not verify a remembered managed workspace before exporting: {error}"
                    ));
                }
            };
            if target == protected_workspace
                || target.starts_with(&protected_workspace)
                || protected_workspace.starts_with(&target)
            {
                return Err(
                    "Choose an export destination outside every managed workspace and outside the original project. No file was changed."
                        .to_owned(),
                );
            }

            let Some(remembered_project) = remembered.project_root() else {
                continue;
            };
            let protected_project = match fs::canonicalize(remembered_project) {
                Ok(path) => path,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(format!(
                        "Mesh could not verify a remembered original project before exporting: {error}"
                    ));
                }
            };
            if target == protected_project
                && project
                    .as_ref()
                    .is_some_and(|project| protected_project == *project)
            {
                targets_original = true;
                continue;
            }
            if target == protected_project
                || target.starts_with(&protected_project)
                || protected_project.starts_with(&target)
            {
                return Err(
                    "Choose an export destination outside every managed workspace and outside the original project. No file was changed."
                        .to_owned(),
                );
            }
        }
        Ok(targets_original)
    }

    fn with_verified_export_workspace<T, F>(
        runtime: &DesktopRuntime,
        target_root: &str,
        expected_workspace_root: &str,
        expected_workspace_digest: &str,
        expected_workspace_installation: &str,
        operation: F,
    ) -> Result<T, String>
    where
        F: FnOnce(bool) -> Result<T, mesh_daemon::ManagedTextFileError>,
    {
        let targets_original =
            export_targets_original_project(&runtime.recent, expected_workspace_root, target_root)?;
        let result = if targets_original {
            runtime.daemon.with_verified_shared_managed_workspace(
                expected_workspace_root,
                expected_workspace_digest,
                expected_workspace_installation,
                || operation(true),
            )
        } else {
            runtime.daemon.with_verified_managed_workspace(
                expected_workspace_root,
                expected_workspace_digest,
                expected_workspace_installation,
                || operation(false),
            )
        };
        result.map_err(|error| error.to_string())
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    fn export_managed_directory(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        target_root: String,
        expected_source_directory_installation: String,
        expected_target_installation: String,
        expected_target_parent_installation: String,
        expected_target_directory_installation: Option<String>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let exported = with_verified_export_workspace(
            &runtime,
            &target_root,
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
            |targets_original| {
                let export = if targets_original {
                    LiveDaemon::export_shared_managed_directory
                } else {
                    LiveDaemon::export_managed_directory
                };
                export(
                    &runtime.daemon,
                    &relative_path,
                    std::path::Path::new(&target_root),
                    &expected_source_directory_installation,
                    &expected_target_installation,
                    &expected_target_parent_installation,
                    expected_target_directory_installation.as_deref(),
                )
            },
        )?;
        Ok(managed_directory_export_json(&exported).encode())
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    fn export_managed_directories(
        runtime: State<'_, DesktopRuntime>,
        paths: Vec<String>,
        target_root: String,
        expected_target_installation: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let (completed, failure) = with_verified_export_workspace(
            &runtime,
            &target_root,
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
            |targets_original| {
                let current = runtime
                    .daemon
                    .preview_managed_directory_exports(std::path::Path::new(&target_root))?;
                if current.target_installation() != expected_target_installation
                    || current.missing_paths() != paths
                {
                    return Err(mesh_daemon::ManagedTextFileError::StaleExportTarget);
                }
                let mut completed = Vec::new();
                let mut failure = None;
                for path in &paths {
                    let exported = runtime
                        .daemon
                        .preview_managed_directory_export(path, std::path::Path::new(&target_root))
                        .and_then(|preview| {
                            let export = if targets_original {
                                LiveDaemon::export_shared_managed_directory
                            } else {
                                LiveDaemon::export_managed_directory
                            };
                            export(
                                &runtime.daemon,
                                preview.path(),
                                std::path::Path::new(&target_root),
                                preview.source_directory_installation(),
                                preview.target_installation(),
                                preview.target_parent_installation(),
                                preview.target_directory_installation(),
                            )
                        });
                    match exported {
                        Ok(exported) => completed.push(exported),
                        Err(error) => {
                            failure = Some((path.clone(), error.to_string()));
                            break;
                        }
                    }
                }
                Ok((completed, failure))
            },
        )?;
        Ok(Json::object([
            (
                "completed",
                Json::Array(
                    completed
                        .iter()
                        .map(managed_directory_export_json)
                        .collect(),
                ),
            ),
            (
                "failure",
                failure.map_or(Json::Null, |(path, error)| {
                    Json::object([("path", Json::text(path)), ("error", Json::text(error))])
                }),
            ),
        ])
        .encode())
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    fn export_managed_file(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        target_root: String,
        expected_target_installation: String,
        expected_target_parent_installation: String,
        expected_target_file_installation: Option<String>,
        expected_source_version: String,
        expected_source_digest: String,
        expected_source_executable: bool,
        expected_target_digest: Option<String>,
        expected_target_executable: Option<bool>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let source_digest = ManagedContentDigest::parse_hex(&expected_source_digest)
            .map_err(|_| "The saved export digest is invalid".to_owned())?;
        let target_digest = expected_target_digest
            .as_deref()
            .map(ManagedContentDigest::parse_hex)
            .transpose()
            .map_err(|_| "The destination export digest is invalid".to_owned())?;
        let exported = with_verified_export_workspace(
            &runtime,
            &target_root,
            &expected_workspace_root,
            &expected_workspace_digest,
            &expected_workspace_installation,
            |targets_original| {
                let export = if targets_original {
                    LiveDaemon::export_shared_managed_file
                } else {
                    LiveDaemon::export_managed_file
                };
                export(
                    &runtime.daemon,
                    &relative_path,
                    std::path::Path::new(&target_root),
                    &expected_target_installation,
                    &expected_target_parent_installation,
                    expected_target_file_installation.as_deref(),
                    &expected_source_version,
                    source_digest,
                    expected_source_executable,
                    target_digest,
                    expected_target_executable,
                )
            },
        )?;
        Ok(Json::object([
            ("path", Json::text(exported.path())),
            ("target_root", Json::text(exported.target_root())),
            ("byte_count", Json::Number(exported.byte_count())),
            (
                "content_digest",
                Json::text(exported.content_digest().to_string()),
            ),
            ("executable", Json::Bool(exported.executable())),
            ("created", Json::Bool(exported.created())),
        ])
        .encode())
    }

    fn managed_entry_change_json(change: &ManagedEntryChange) -> String {
        Json::object([
            ("action", Json::text(change.action())),
            (
                "from_path",
                change.from_path().map_or(Json::Null, Json::text),
            ),
            ("to_path", change.to_path().map_or(Json::Null, Json::text)),
            ("changeset", Json::text(change.changeset())),
            ("saved_privately", Json::Bool(change.meaningful_saved())),
            (
                "author_authenticated",
                Json::Bool(change.author_authenticated()),
            ),
            (
                "state",
                Json::text(if change.meaningful_saved() {
                    "saved_privately"
                } else {
                    "working"
                }),
            ),
        ])
        .encode()
    }

    #[tauri::command(async)]
    fn create_managed_text(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        text: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let saved = with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let author = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local author key is unavailable".to_owned())?;
                let public_key = author.public_key().public_key();
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.create_managed_text_file(
                                &relative_path,
                                &text,
                                public_key,
                                |payload| author.sign(payload),
                            )
                        },
                    )
                    .map_err(|error| error.to_string())
            },
        )?;
        Ok(Json::object([
            ("action", Json::text("create_text")),
            ("from_path", Json::Null),
            ("to_path", Json::text(saved.path())),
            ("version", Json::text(saved.version())),
            ("manifest", Json::text(saved.manifest())),
            ("changeset", Json::text(saved.changeset())),
            ("saved_privately", Json::Bool(saved.meaningful_saved())),
            (
                "author_authenticated",
                Json::Bool(saved.author_authenticated()),
            ),
            (
                "state",
                Json::text(if saved.meaningful_saved() {
                    "saved_privately"
                } else {
                    "working"
                }),
            ),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn create_managed_folder(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let author = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local author key is unavailable".to_owned())?;
                let public_key = author.public_key().public_key();
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.create_managed_folder(
                                &relative_path,
                                public_key,
                                |payload| author.sign(payload),
                            )
                        },
                    )
                    .map(|change| managed_entry_change_json(&change))
                    .map_err(|error| error.to_string())
            },
        )
    }

    #[tauri::command(async)]
    fn move_managed_entry(
        runtime: State<'_, DesktopRuntime>,
        from_path: String,
        to_path: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let author = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local author key is unavailable".to_owned())?;
                let public_key = author.public_key().public_key();
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.move_managed_entry_privately(
                                &from_path,
                                &to_path,
                                public_key,
                                |payload| author.sign(payload),
                            )
                        },
                    )
                    .map(|change| managed_entry_change_json(&change))
                    .map_err(|error| error.to_string())
            },
        )
    }

    #[tauri::command(async)]
    fn delete_managed_entry(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        expected_content_digest: Option<String>,
        expected_executable: Option<bool>,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let expected_content_digest = expected_content_digest
            .map(|digest| ManagedContentDigest::parse_hex(&digest))
            .transpose()
            .map_err(|error| format!("the inspected content digest is invalid: {error}"))?;
        with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let author = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local author key is unavailable".to_owned())?;
                let public_key = author.public_key().public_key();
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.delete_managed_entry_privately(
                                &relative_path,
                                expected_content_digest,
                                expected_executable,
                                public_key,
                                |payload| author.sign(payload),
                            )
                        },
                    )
                    .map(|change| managed_entry_change_json(&change))
                    .map_err(|error| error.to_string())
            },
        )
    }

    #[tauri::command(async)]
    fn adopt_native_file_deletion(
        runtime: State<'_, DesktopRuntime>,
        relative_path: String,
        expected_current_version: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let author = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local author key is unavailable".to_owned())?;
                let public_key = author.public_key().public_key();
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.adopt_native_file_deletion_privately(
                                &relative_path,
                                &expected_current_version,
                                public_key,
                                |payload| author.sign(payload),
                            )
                        },
                    )
                    .map(|change| managed_entry_change_json(&change))
                    .map_err(|error| error.to_string())
            },
        )
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    fn adopt_native_file_move(
        runtime: State<'_, DesktopRuntime>,
        from_path: String,
        to_path: String,
        expected_current_version: String,
        expected_destination_content_digest: String,
        expected_destination_executable: bool,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let expected_destination_content_digest =
            ManagedContentDigest::parse_hex(&expected_destination_content_digest)
                .map_err(|error| format!("the inspected destination digest is invalid: {error}"))?;
        with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                let author = runtime
                    .author
                    .lock()
                    .map_err(|_| "The local author key is unavailable".to_owned())?;
                let public_key = author.public_key().public_key();
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.adopt_native_file_move_privately(
                                &from_path,
                                &to_path,
                                &expected_current_version,
                                expected_destination_content_digest,
                                expected_destination_executable,
                                public_key,
                                |payload| author.sign(payload),
                            )
                        },
                    )
                    .map(|change| managed_entry_change_json(&change))
                    .map_err(|error| error.to_string())
            },
        )
    }

    #[tauri::command(async)]
    // Keep the inspected file state and verified workspace state as explicit named Tauri fields.
    #[allow(clippy::too_many_arguments)]
    fn restore_managed_version(
        runtime: State<'_, DesktopRuntime>,
        object_id: String,
        target_version: String,
        expected_content_digest: String,
        expected_executable: bool,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        let expected_content_digest = ManagedContentDigest::parse_hex(&expected_content_digest)
            .map_err(|_| "The inspected managed-file digest is invalid".to_owned())?;
        let restored = with_unassigned_managed_workspace(
            &runtime.recent,
            &expected_workspace_root,
            &expected_workspace_installation,
            || {
                runtime
                    .daemon
                    .with_verified_managed_workspace(
                        &expected_workspace_root,
                        &expected_workspace_digest,
                        &expected_workspace_installation,
                        || {
                            runtime.daemon.restore_managed_file_version(
                                &object_id,
                                &target_version,
                                expected_content_digest,
                                expected_executable,
                            )
                        },
                    )
                    .map_err(|error| error.to_string())
            },
        )?;
        Ok(Json::object([
            ("path", Json::text(restored.path())),
            ("target_version", Json::text(restored.target_version())),
            (
                "content_digest",
                Json::text(restored.content_digest().to_string()),
            ),
            ("executable", Json::Bool(restored.executable())),
            ("recovery", Json::text(restored.recovery().to_string())),
            (
                "stable_after_idle",
                Json::Bool(restored.stable_after_idle()),
            ),
            ("recovery_preserved", Json::Bool(true)),
            ("saved_privately", Json::Bool(false)),
            ("state", Json::text("working")),
        ])
        .encode())
    }

    #[tauri::command(async)]
    fn preview_managed_restore(
        runtime: State<'_, DesktopRuntime>,
        object_id: String,
        target_version: String,
        expected_workspace_root: String,
        expected_workspace_digest: String,
        expected_workspace_installation: String,
    ) -> Result<String, String> {
        runtime
            .daemon
            .preview_managed_working_copy_restore_for_workspace(
                &expected_workspace_root,
                &expected_workspace_digest,
                &expected_workspace_installation,
                &object_id,
                &target_version,
            )
            .map(|preview| preview.encode())
            .map_err(|error| error.to_string())
    }

    #[tauri::command(async)]
    fn managed_checkpoint_state(runtime: State<'_, DesktopRuntime>) -> Result<String, String> {
        managed_checkpoint_state_for(&runtime.daemon)
    }

    fn managed_checkpoint_state_for(daemon: &LiveDaemon) -> Result<String, String> {
        let (root, workspace_digest, workspace_installation, snapshot) = daemon
            .checkpoint_snapshot_for_open_workspace()
            .map_err(|error| error.to_string())?;
        // Native workspace identity is independent of the optional stable Finder link. A link can
        // be unavailable while the exact presented folder remains healthy and safe for a pinned
        // terminal or agent. Bind that fact to the same root/digest/installation tuple as this
        // checkpoint response so the browser never guesses from link availability or path shape.
        let verified = daemon
            .verified_managed_workspace_path(&root, &workspace_digest, &workspace_installation)
            .map_err(|error| error.to_string())?;
        let native_folder = verified.is_presented();
        // A valid confirmed-import receipt is only a navigation-recovery hint for the browser.
        // It cannot authorize adoption by itself: import_managed_workspace_for still requires the
        // unchanged selected source and every exact private per-object origin receipt before it
        // records an original-project relationship.
        let confirmed_import_receipt =
            native_folder && mesh_daemon::ConfirmedFolderImport::open(verified.path()).is_ok();
        Ok(Json::object([
            ("root", Json::text(root)),
            ("workspace_digest", Json::text(workspace_digest)),
            ("workspace_installation", Json::text(workspace_installation)),
            ("native_folder", Json::Bool(native_folder)),
            (
                "native_folder_path",
                if native_folder {
                    Json::text(verified.path().display().to_string())
                } else {
                    Json::Null
                },
            ),
            (
                "confirmed_import_receipt",
                Json::Bool(confirmed_import_receipt),
            ),
            ("working", Json::Bool(snapshot.open_window().is_some())),
            (
                "recovery_preserved",
                Json::Bool(snapshot.latest_recovery().is_some()),
            ),
            (
                "meaningful_saved",
                Json::Bool(snapshot.last_meaningful().is_some()),
            ),
            (
                "through",
                snapshot
                    .open_window()
                    .map_or(Json::Null, |window| Json::Number(window.last().get())),
            ),
        ])
        .encode())
    }

    fn read_message(
        reader: &mut BufReader<UnixStream>,
        chunks: &mut ChunkAssembler,
    ) -> Result<DaemonMessage, String> {
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).map_err(|error| {
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) {
                    "The local service did not answer before the desktop reply deadline".to_owned()
                } else {
                    error.to_string()
                }
            })?;
            if line.is_empty() {
                return Err("The local service closed before answering".to_owned());
            }
            let frame = DaemonMessage::decode(line.trim_end_matches(['\r', '\n']))
                .map_err(|error| error.to_string())?;
            if let Some(message) = chunks.push(frame).map_err(|error| error.to_string())? {
                return Ok(message);
            }
        }
    }

    pub fn run() {
        let app = tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .on_window_event(|window, event| {
                #[cfg(not(target_os = "macos"))]
                let _ = (window, event);
                #[cfg(target_os = "macos")]
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    // Keep the one configured window alive when the macOS close button is used.
                    // Otherwise the daemon process remains healthy but a later Dock/Finder open
                    // has no window to reveal, which makes Mesh appear to have stopped working.
                    api.prevent_close();
                    let _ = window.hide();
                }
            })
            .invoke_handler(tauri::generate_handler![
                pick_folder,
                attach_existing_project,
                attached_projects,
                attached_fleets,
                provision_attached_fleet,
                fleet_activity,
                remote_fleet_reviews,
                inspect_remote_fleet_review,
                render_remote_fleet_artifact,
                fleet_saved_reviews,
                inspect_fleet_saved_review,
                fleet_project_mapping,
                prepare_fleet_project_candidate,
                import_fleet_project_candidate,
                review_imported_fleet_project_candidate,
                review_fleet_project_candidate,
                render_fleet_review_artifact,
                request_fleet_review_changes,
                decide_fleet_review_change,
                fleet_review_changes,
                inspect_fleet_starting_comparison,
                start_attached_fleet,
                stop_attached_fleet,
                attached_project_versions,
                inspect_attached_version,
                compare_attached_versions,
                request_attached_review,
                attached_project_reviews,
                inspect_attached_review,
                attachment_approval_status,
                approve_attached_review,
                preview_attached_main_integration,
                inspect_attached_recovery,
                open_attached_version_lane,
                open_attached_folder,
                restore_attached_retained_file,
                restore_attached_retained_entry,
                restore_attached_group_file,
                apply_attached_main_file,
                apply_attached_main_group,
                inspect_attached_group_recovery,
                inspect_attached_group_file,
                load_attachment_pins,
                save_attachment_pins,
                load_fleet_review_outbox,
                save_fleet_review_outbox,
                load_fleet_pins,
                save_fleet_pins,
                compare_attached_path,
                control_attached_project,
                recent_workspace_status,
                renderer_proof_configuration,
                renderer_proof_capture_files_screenshot,
                renderer_proof_report,
                renderer_proof_accept_private_export_confirmation,
                renderer_proof_failure,
                renderer_proof_checkpoint,
                renderer_proof_agent_handoff_rescanned,
                open_current_review,
                render_review_artifact,
                export_review_artifact_inspection,
                open_review_artifact_inspection,
                approval_credential_status,
                enroll_approval_credential,
                approve_current_review,
                export_shared_review_to_git,
                inspect_shared_review_git_export,
                remember_managed_workspace,
                forget_managed_workspace,
                reveal_managed_workspace,
                open_managed_workspace_entry,
                reconcile_managed_workspace_navigation,
                reconcile_current_workspace_navigation,
                prepare_managed_workspace_agent_path,
                open_managed_workspace_in_codex,
                open_managed_workspace_in_terminal,
                finish_managed_workspace_agent_handoff,
                preview_managed_workspace_version,
                open_managed_workspace_version,
                open_fresh_agent_workspace,
                import_managed_workspace,
                rollback_managed_workspace,
                daemon_call,
                native_capture_preference,
                set_native_capture_preference,
                inspect_agent_finish_preflight,
                inspect_agent_live_work,
                inspect_agent_live_file,
                inspect_managed_file,
                inspect_native_file,
                inspect_managed_directory_installation,
                discover_native_missing_files,
                discover_native_directories,
                preserve_managed_text,
                save_managed_private,
                adopt_native_file,
                adopt_native_directory,
                discover_retired_exports,
                preview_retired_export,
                remove_retired_export,
                preview_managed_export,
                preview_managed_exports,
                preview_managed_directory_export,
                preview_managed_directory_exports,
                export_managed_file,
                export_managed_directory,
                export_managed_directories,
                create_managed_text,
                create_managed_folder,
                move_managed_entry,
                delete_managed_entry,
                adopt_native_file_deletion,
                adopt_native_file_move,
                preview_managed_restore,
                restore_managed_version,
                managed_checkpoint_state
            ])
            .setup(|app| {
                let app_data_dir = app
                    .path()
                    .app_local_data_dir()
                    .map_err(|error| error.to_string())?;
                let runtime_dir = desktop_runtime_directory(&app_data_dir)?;
                fs::create_dir_all(&runtime_dir)?;
                let endpoint = runtime_dir.join(DAEMON_SOCKET_NAME);
                let pending_server = match bind_desktop_endpoint(&endpoint)? {
                    DesktopEndpoint::Owned(server) => server,
                    DesktopEndpoint::AlreadyOwned => {
                        // Never initialize a second local authority or touch its navigation files.
                        // Finder normally reactivates the first process, but a freshly extracted
                        // build can be launched directly while an older copy is still running.
                        // Keep that upgrade mistake recoverable and visible instead of returning a
                        // setup error that Tauri turns into a non-unwinding process panic.
                        if request_existing_desktop_attention(&runtime_dir).is_ok() {
                            eprintln!(
                                "Mesh is already open; the running window was asked to come forward"
                            );
                            std::process::exit(0);
                        }
                        eprintln!(
                            "the local endpoint is already owned by another process; Mesh is already open. Use the running window, or quit every Mesh copy before switching builds"
                        );
                        app.dialog()
                            .message(
                                "Another Mesh copy is already running, but it is too old to bring its window forward automatically.\n\nUse the running Mesh window, or quit every Mesh copy completely before opening this build. No workspace data changed.",
                            )
                            .title("Mesh is already open")
                            .buttons(MessageDialogButtons::Ok)
                            .blocking_show();
                        std::process::exit(2);
                    }
                };
                let attention_app = app.handle().clone();
                let attention = DesktopAttentionServer::bind(
                    &runtime_dir,
                    Arc::new(move || {
                        let app = attention_app.clone();
                        let reveal = app.clone();
                        let _ = app.run_on_main_thread(move || reveal_desktop_window(&reveal));
                    }),
                )?;
                let startup = StartupSummary::from(&nothing_to_recover());
                let trusted_reviewers = SecureEnclaveApprovalCredential::load().map_or_else(
                    |_| mesh_daemon::TrustedReviewers::default(),
                    |credential| {
                        mesh_daemon::TrustedReviewers::with_human_credentials([credential
                            .credential()
                            .clone()])
                    },
                );
                let daemon = Arc::new(LiveDaemon::with_trusted_reviewers_and_checkpoint_runtime(
                    startup,
                    trusted_reviewers,
                    CheckpointRuntimeParameters {
                        idle_interval: Some(Duration::from_millis(50)),
                        maximum_uncheckpointed_bytes: Some(65_536),
                        maximum_uncheckpointed_interval: Some(Duration::from_millis(25)),
                    },
                )?);
                let active_workspace = ActiveWorkspaceLink::new(&app_data_dir);
                let version_workspaces = VersionWorkspaceDirectory::new(&app_data_dir);
                let native_capture = NativeCapturePreference::new(&app_data_dir);
                app.manage(Arc::new(AttachmentHost::new(&app_data_dir)));
                app.manage(Arc::new(crate::fleet_host::FleetHosts::default()));
                let recent = RecentWorkspace::new(app_data_dir);
                let recent_status =
                    reopen_remembered_workspace(&daemon, &recent, &active_workspace);
                let _ = daemon.schedule_pending_checkpoint();
                let author = SoftwareActorCustody::generate()?;
                let operations: Arc<dyn Operations> = daemon.clone();
                let server = pending_server.spawn(operations)?;
                app.manage(attention);
                app.manage(DesktopRuntime {
                    endpoint,
                    daemon,
                    author: Mutex::new(author),
                    active_workspace,
                    version_workspaces,
                    native_capture,
                    recent,
                    recent_status: Mutex::new(recent_status),
                    renderer_proof: RendererProofRuntime::from_environment(),
                    _server: Mutex::new(Some(server)),
                });
                Ok(())
            })
            .build(tauri::generate_context!())
            .expect("Mesh desktop failed to build");

        app.run(|app_handle, event| {
            #[cfg(not(target_os = "macos"))]
            let _ = (app_handle, event);
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                // `window.hide()` can leave AppKit treating the process itself as hidden. In
                // that state ordering the webview front is not enough: Finder/Dock reopen keeps
                // a healthy daemon alive with no on-screen window. Unhide the application first,
                // and do this for every reopen event because AppKit's visible-window hint can
                // race the prevent-close/hide transition.
                reveal_desktop_window(app_handle);
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use ring::rand::SystemRandom;
        use ring::signature::{EcdsaKeyPair, KeyPair as _, ECDSA_P256_SHA256_ASN1_SIGNING};
        use std::os::unix::net::UnixListener;
        use std::thread;

        #[test]
        fn artifact_preview_encoding_keeps_saved_identity_and_no_approval_authority() {
            use crate::artifact_preview::{ArtifactKind, ArtifactPreview, ArtifactRenderer};
            let image = ArtifactPreview {
                kind: ArtifactKind::Png,
                png: b"preview".to_vec(),
                text: None,
                renderer: ArtifactRenderer::ImageIoThumbnail,
                page_number: None,
                page_count: None,
            };
            let version = "a".repeat(64);
            let digest = "b".repeat(64);
            let value = rendered_artifact_json(&image, "before", &version, &digest);
            assert_eq!(
                value.get("version_id").and_then(Json::as_text),
                Some(version.as_str())
            );
            assert_eq!(
                value.get("content_digest").and_then(Json::as_text),
                Some(digest.as_str())
            );
            assert_eq!(value.get("side").and_then(Json::as_text), Some("before"));
            assert_eq!(
                value.get("image_data_url").and_then(Json::as_text),
                Some("data:image/png;base64,cHJldmlldw==")
            );
            assert_eq!(
                value.get("rendering_authorizes_approval"),
                Some(&Json::Bool(false))
            );
            assert_eq!(value.get("text_lines"), Some(&Json::Null));
            assert_eq!(value.get("page_number"), Some(&Json::Null));
            let page = ArtifactPreview {
                kind: ArtifactKind::Pdf,
                renderer: ArtifactRenderer::PdfKitPage,
                page_number: Some(3),
                page_count: Some(8),
                ..image
            };
            let value = rendered_artifact_json(&page, "after", &version, &digest);
            assert_eq!(
                value.get("scope").and_then(Json::as_text),
                Some("exact-page-preview")
            );
            assert_eq!(value.get("page_number"), Some(&Json::Number(3)));
            assert_eq!(value.get("page_count"), Some(&Json::Number(8)));
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn renderer_files_screenshot_is_private_bounded_png() {
            let root = std::env::temp_dir().join(format!(
                "mesh-renderer-screenshot-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("current time")
                    .as_nanos()
            ));
            let application_data = root.join("application-data");
            fs::create_dir_all(&application_data).expect("private application data");
            fs::set_permissions(&application_data, fs::Permissions::from_mode(0o700))
                .expect("private application data permissions");
            let (path, width, height, bytes, sha256) = persist_renderer_files_screenshot(
                &application_data,
                "files-abababababababababababababababababababababababababababababababab.png",
                include_bytes!("icons/icon.png"),
            )
            .expect("app-owned screenshot conversion");
            let metadata = fs::metadata(&path).expect("screenshot metadata");
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            assert!((64..=8_192).contains(&width));
            assert!((64..=8_192).contains(&height));
            assert_eq!(metadata.len(), bytes);
            assert_eq!(sha256.len(), 64);
            assert!(fs::read(&path)
                .expect("screenshot bytes")
                .starts_with(b"\x89PNG\r\n\x1a\n"));
            assert!(!path.with_extension("tiff").exists());
            fs::remove_dir_all(&root).expect("screenshot cleanup");
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn renderer_files_screenshot_never_follows_replaced_paths() {
            use std::os::unix::fs::symlink;

            let root = std::env::temp_dir().join(format!(
                "mesh-renderer-screenshot-race-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("current time")
                    .as_nanos()
            ));
            let application_data = root.join("application-data");
            let outside = root.join("outside");
            fs::create_dir_all(&application_data).expect("private application data");
            fs::create_dir_all(&outside).expect("outside directory");
            fs::set_permissions(&application_data, fs::Permissions::from_mode(0o700))
                .expect("private application data permissions");
            let proof = application_data.join("renderer-proof");
            let screenshot_name =
                "files-cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd.png";
            let victim = outside.join("victim.png");

            symlink(&outside, &proof).expect("replacement proof root");
            assert!(persist_renderer_files_screenshot(
                &application_data,
                screenshot_name,
                include_bytes!("icons/icon.png"),
            )
            .is_err());
            assert!(fs::read_dir(&outside)
                .expect("outside listing")
                .next()
                .is_none());
            fs::remove_file(&proof).expect("remove replacement proof root");

            fs::create_dir(&proof).expect("real proof root");
            fs::set_permissions(&proof, fs::Permissions::from_mode(0o700))
                .expect("proof root permissions");
            symlink(&victim, proof.join(screenshot_name)).expect("dangling output symlink");
            assert!(persist_renderer_files_screenshot(
                &application_data,
                screenshot_name,
                include_bytes!("icons/icon.png"),
            )
            .is_err());
            assert!(!victim.exists());
            fs::remove_file(proof.join(screenshot_name)).expect("remove dangling output symlink");

            fs::write(proof.join(screenshot_name), b"preexisting user bytes")
                .expect("preexisting output");
            assert!(persist_renderer_files_screenshot(
                &application_data,
                screenshot_name,
                include_bytes!("icons/icon.png"),
            )
            .is_err());
            assert_eq!(
                fs::read(proof.join(screenshot_name)).expect("preserved output"),
                b"preexisting user bytes"
            );
            fs::remove_dir_all(&root).expect("screenshot race cleanup");
        }

        struct TestApprovalSigner {
            key: EcdsaKeyPair,
            credential: mesh_approval::HumanApprovalCredential,
        }

        impl TestApprovalSigner {
            fn generate() -> Self {
                let random = SystemRandom::new();
                let document =
                    EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random)
                        .expect("test P-256 key");
                let key = EcdsaKeyPair::from_pkcs8(
                    &ECDSA_P256_SHA256_ASN1_SIGNING,
                    document.as_ref(),
                    &random,
                )
                .expect("parse test P-256 key");
                let public_key: [u8; 65] = key
                    .public_key()
                    .as_ref()
                    .try_into()
                    .expect("uncompressed P-256 key");
                let credential =
                    mesh_approval::HumanApprovalCredential::from_public_key(public_key)
                        .expect("test approval credential");
                Self { key, credential }
            }

            fn sign(&self, expected: ExpectedHumanApproval) -> Vec<u8> {
                let draft = HumanApprovalReceiptDraft::new(expected, ApprovalDecision::Approve);
                let signature = self
                    .key
                    .sign(&SystemRandom::new(), &draft.canonical_bytes())
                    .expect("test approval signature");
                draft
                    .with_signature(signature.as_ref().to_vec())
                    .expect("canonical test approval")
                    .canonical_bytes()
            }
        }

        fn daemon() -> LiveDaemon {
            LiveDaemon::with_checkpoint_runtime(
                StartupSummary::from(&nothing_to_recover()),
                CheckpointRuntimeParameters {
                    idle_interval: Some(Duration::from_millis(50)),
                    maximum_uncheckpointed_bytes: Some(65_536),
                    maximum_uncheckpointed_interval: Some(Duration::from_millis(25)),
                },
            )
            .expect("daemon")
        }

        #[cfg(feature = "git-integration")]
        #[test]
        fn shared_review_git_export_rechecks_agent_custody_after_confirmation() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-git-export-custody-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let source = scratch.join("source");
            let private_workspace = scratch.join("workspace.mesh");
            let target = scratch.join("target");
            fs::create_dir_all(&source).expect("source folder");
            fs::write(source.join("notes.txt"), "approved notes\n").expect("source file");
            let prepared =
                mesh_daemon::PreparedFolderImport::prepare_presented(&source, &private_workspace)
                    .expect("preview import");
            let (confirmed, imported) = prepared.confirm_into_workspace().expect("confirm import");
            let presented = confirmed.destination().to_path_buf();
            drop(confirmed);

            let signer = TestApprovalSigner::generate();
            let trust =
                mesh_daemon::TrustedReviewers::with_human_credentials([signer.credential.clone()]);
            let daemon = LiveDaemon::with_trusted_reviewers(
                StartupSummary::from(&nothing_to_recover()),
                trust.clone(),
            );
            daemon
                .open_at_start(&presented)
                .expect("open imported workspace");
            let shown = Operations::workspace_state(&daemon).expect("workspace state");
            let opened_by = SoftwareActorCustody::generate()
                .expect("review actor")
                .public_key()
                .public_key();
            let reviewed = daemon
                .open_current_review_for_workspace(
                    &shown.root,
                    &shown.digest,
                    &shown.installation,
                    opened_by,
                )
                .expect("record review");
            let review = reviewed.review_items.first().expect("review item");
            let bundle = review
                .get("bundle")
                .and_then(Json::as_text)
                .expect("review bundle")
                .to_owned();
            let target_operation = imported.operation().to_string();
            let context = daemon
                .human_approval_context_for_workspace(
                    &reviewed.root,
                    &reviewed.digest,
                    &reviewed.installation,
                    &bundle,
                    &target_operation,
                )
                .expect("approval context");
            let receipt = signer.sign(ExpectedHumanApproval::new(
                context,
                signer.credential.clone(),
                [7; 32],
            ));
            let approved = Operations::approve_review(
                &daemon,
                &bundle,
                &target_operation,
                &encode_hex(&receipt),
            )
            .expect("approve review");
            let durable = daemon
                .durable_human_approval_for_workspace(
                    &approved.root,
                    &approved.digest,
                    &approved.installation,
                    &bundle,
                    &target_operation,
                )
                .expect("durable approval");

            fs::create_dir_all(&target).expect("Git target");
            let git = |arguments: &[&str]| {
                let output = Command::new("git")
                    .arg("-C")
                    .arg(&target)
                    .args(arguments)
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .output()
                    .expect("run Git fixture command");
                assert!(
                    output.status.success(),
                    "git {arguments:?}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                output.stdout
            };
            git(&["init", "-b", "main"]);
            git(&["config", "user.name", "Mesh test"]);
            git(&["config", "user.email", "mesh@example.invalid"]);
            fs::write(target.join("baseline.txt"), "baseline\n").expect("target baseline");
            git(&["add", "baseline.txt"]);
            git(&["commit", "-m", "baseline"]);
            let target_anchor =
                mesh_git_bridge::GitProvenanceAnchor::inspect(&target).expect("target anchor");
            let actors = [durable.preview().review_bundle().author()];
            let export_source = mesh_git_bridge::ApprovedGitExportSource::new(
                durable.receipt(),
                durable.expected(),
                durable.preview().review_bundle(),
                durable.preview().approved_state(),
                &actors,
            );
            let export_preview = mesh_git_bridge::preview_approved_git_export(
                Path::new(&approved.root),
                &target,
                &target_anchor,
                &export_source,
            )
            .expect("preview approved export");

            let competing = LiveDaemon::with_trusted_reviewers(
                StartupSummary::from(&nothing_to_recover()),
                trust,
            );
            competing
                .open_at_start(&presented)
                .expect("second daemon opens workspace");
            let generation = competing
                .acquire_workspace_agent_custody(
                    &approved.root,
                    &approved.digest,
                    &approved.installation,
                    false,
                    None,
                )
                .expect("agent acquires custody while confirmation is open");
            let refusal = confirm_shared_review_git_export(
                &daemon,
                &approved.root,
                &approved.digest,
                &approved.installation,
                &export_preview,
                &target_anchor,
                &export_source,
            )
            .expect_err("stale confirmation must not export during agent custody");
            assert!(refusal.contains("agent"), "{refusal}");
            let branch = format!("refs/heads/{}", export_preview.branch());
            let approval_ref = format!("refs/mesh/approvals/{}", export_preview.approval());
            for reference in [&branch, &approval_ref] {
                let status = Command::new("git")
                    .arg("-C")
                    .arg(&target)
                    .args(["rev-parse", "--verify", reference])
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .output()
                    .expect("inspect refused ref")
                    .status;
                assert!(!status.success(), "refusal created {reference}");
            }

            assert!(competing
                .release_workspace_agent_custody(
                    &approved.root,
                    &approved.digest,
                    &approved.installation,
                    &generation,
                )
                .expect("release exact custody"));
            let exported = confirm_shared_review_git_export(
                &daemon,
                &approved.root,
                &approved.digest,
                &approved.installation,
                &export_preview,
                &target_anchor,
                &export_source,
            )
            .expect("exact retry after release");
            assert_eq!(exported.branch(), export_preview.branch());
            for reference in [&branch, &approval_ref] {
                let status = Command::new("git")
                    .arg("-C")
                    .arg(&target)
                    .args(["rev-parse", "--verify", reference])
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .output()
                    .expect("inspect installed ref")
                    .status;
                assert!(status.success(), "retry did not create {reference}");
            }
            fs::remove_dir_all(scratch).expect("fixture cleanup");
        }

        #[test]
        fn recent_status_reports_the_build_identity_without_inventing_an_exact_revision() {
            let encoded = RecentWorkspaceStatus::empty().to_json();
            assert!(encoded.contains("\"build_revision\":\"development\""));
            assert!(encoded.contains("\"build_exact\":false"));
        }

        #[test]
        fn a_long_application_data_path_uses_a_stable_short_owner_runtime() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-long-application-data-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let long_app_data = scratch.join("account-name-and-managed-home-redirect-".repeat(4));
            fs::create_dir_all(&long_app_data).expect("long application data directory");

            let preferred = long_app_data.join("runtime").join(DAEMON_SOCKET_NAME);
            assert!(
                preferred.as_os_str().len() > MAX_LOCAL_ENDPOINT_BYTES,
                "fixture must exceed the macOS socket-path limit: {}",
                preferred.display()
            );
            let first = desktop_runtime_directory(&long_app_data).expect("short runtime fallback");
            let second =
                desktop_runtime_directory(&long_app_data).expect("stable runtime fallback");
            assert_eq!(first, second);
            assert_ne!(first, long_app_data.join("runtime"));
            assert_eq!(
                first.parent(),
                Some(
                    fs::canonicalize("/tmp")
                        .expect("real temporary root")
                        .as_path()
                )
            );
            assert!(first.join(DAEMON_SOCKET_NAME).as_os_str().len() <= MAX_LOCAL_ENDPOINT_BYTES);
            let owner = <fs::Metadata as std::os::unix::fs::MetadataExt>::uid(
                &fs::metadata(&long_app_data).expect("application data metadata"),
            );
            assert_eq!(
                first.file_name().and_then(|name| name.to_str()),
                Some(format!("mesh-desktop-{owner}").as_str())
            );

            fs::remove_dir_all(scratch).expect("fixture cleanup");
        }

        #[test]
        fn a_short_application_data_path_keeps_its_private_runtime_directory() {
            let app_data = Path::new("/a/short/mesh-app-data");
            assert_eq!(
                desktop_runtime_directory(app_data).expect("preferred runtime"),
                app_data.join("runtime")
            );
        }

        #[test]
        fn custom_version_destination_stays_outside_every_remembered_workspace_and_project() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-version-destination-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let application = scratch.join("app");
            let workspace = scratch.join("workspace");
            let original = scratch.join("original");
            let safe_parent = scratch.join("copies");
            for directory in [&application, &workspace, &original, &safe_parent] {
                fs::create_dir_all(directory).expect("fixture directory");
            }
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private application data");
            let recent = RecentWorkspace::new(application.clone());
            recent
                .remember_with_navigation_hints(&workspace, Some(&original), Some(&original))
                .expect("remember workspace and original project");
            recent
                .record_agent_handoff(
                    &workspace,
                    "blake3:test-workspace-installation",
                    &ProtectedWorkspaceRoot::inspect(&workspace)
                        .expect("workspace directory identity")
                        .directory_token(),
                )
                .expect("record agent handoff");

            for refused in [
                workspace.join("nested-version.mesh"),
                original.join("nested-version.mesh"),
            ] {
                let error = ensure_custom_version_destination_is_independent(
                    &recent,
                    refused.to_str().expect("UTF-8 destination"),
                )
                .expect_err("overlapping custom version location");
                assert!(error.contains("No folder was created"), "{error}");
                assert!(!refused.exists());
            }
            let displaced_workspace = scratch.join("agent-workspace-displaced");
            fs::rename(&workspace, &displaced_workspace).expect("move assigned workspace");
            fs::create_dir(&workspace).expect("replacement at remembered path");
            let protected = ensure_custom_version_destination_is_independent(
                &recent,
                safe_parent
                    .join("independent-version.mesh")
                    .to_str()
                    .expect("UTF-8 safe destination"),
            )
            .expect("independent sibling destination");
            assert_eq!(
                protected.len(),
                3,
                "the displaced assigned workspace, replacement path, and project are pinned"
            );

            let legacy_record = application.join("recent-workspace.json");
            fs::write(
                &legacy_record,
                Json::object([
                    ("schema", Json::text("mesh-desktop-recent-workspaces/v7")),
                    (
                        "workspaces",
                        Json::Array(vec![Json::object([
                            (
                                "path",
                                Json::text(
                                    fs::canonicalize(&workspace)
                                        .expect("canonical replacement")
                                        .to_string_lossy(),
                                ),
                            ),
                            ("export_root", Json::Null),
                            (
                                "project_root",
                                Json::text(
                                    fs::canonicalize(&original)
                                        .expect("canonical project")
                                        .to_string_lossy(),
                                ),
                            ),
                            (
                                "agent_handoff_installation",
                                Json::text("blake3:legacy-installation"),
                            ),
                            ("source_point_ordinal", Json::Null),
                            ("original_update_version", Json::Null),
                        ])]),
                    ),
                ])
                .encode(),
            )
            .expect("legacy navigation record");
            fs::set_permissions(&legacy_record, fs::Permissions::from_mode(0o600))
                .expect("owner-only legacy record");
            let error = ensure_custom_version_destination_is_independent(
                &recent,
                safe_parent
                    .join("legacy-refused.mesh")
                    .to_str()
                    .expect("UTF-8 legacy destination"),
            )
            .expect_err("legacy active custody has no exact directory identity");
            assert!(
                error.contains("Finish the legacy agent assignment"),
                "{error}"
            );

            fs::remove_dir_all(scratch).expect("cleanup");
        }

        #[test]
        fn approval_unavailability_is_a_structured_status_not_a_failed_status_call() {
            let encoded =
                approval_status_json(None, Some("validated Apple application identity required"));
            assert!(encoded.contains("\"enrolled\":false"));
            assert!(encoded.contains("\"available\":false"));
            assert!(encoded.contains(
                "\"unavailable_reason\":\"validated Apple application identity required\""
            ));
            assert!(encoded.contains("\"credential_id\":null"));
        }

        #[test]
        fn a_second_desktop_detects_the_existing_owner_without_replacing_it() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-existing-owner-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let endpoint = scratch.join("daemon.sock");

            let owner = match bind_desktop_endpoint(&endpoint).expect("first owner") {
                DesktopEndpoint::Owned(server) => server,
                DesktopEndpoint::AlreadyOwned => panic!("the fresh endpoint had an owner"),
            };
            assert!(matches!(
                bind_desktop_endpoint(&endpoint).expect("second launch classification"),
                DesktopEndpoint::AlreadyOwned
            ));
            assert!(
                endpoint.exists(),
                "the second launch preserved the owner endpoint"
            );

            drop(owner);
            let replacement = match bind_desktop_endpoint(&endpoint).expect("replacement owner") {
                DesktopEndpoint::Owned(server) => server,
                DesktopEndpoint::AlreadyOwned => panic!("the released endpoint stayed owned"),
            };
            drop(replacement);
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn native_export_requires_shared_truth_only_for_the_remembered_original_project() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-original-export-guard-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let application = scratch.join("app");
            let workspace = scratch.join("workspace");
            let original = scratch.join("original");
            let nested_original = original.join("nested-export");
            let original_alias = scratch.join("original-alias");
            let other_workspace = scratch.join("other-workspace");
            let other_project = scratch.join("other-project");
            let another_destination = scratch.join("another-destination");
            for directory in [
                &application,
                &workspace,
                &original,
                &nested_original,
                &other_workspace,
                &other_project,
                &another_destination,
            ] {
                fs::create_dir_all(directory).expect("fixture directory");
            }
            std::os::unix::fs::symlink(&nested_original, &original_alias)
                .expect("symlink alias into the original project");
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private app directory");

            let daemon = Arc::new(daemon());
            let state = daemon
                .open_workspace(&workspace.display().to_string())
                .expect("open workspace");
            let recent = RecentWorkspace::new(application.clone());
            recent
                .remember_with_navigation_hints(
                    std::path::Path::new(&state.root),
                    Some(&original),
                    Some(&original),
                )
                .expect("remember original project association");
            recent
                .remember_with_navigation_hints(
                    &other_workspace,
                    Some(&other_project),
                    Some(&other_project),
                )
                .expect("remember a second managed workspace and project");
            let runtime = DesktopRuntime {
                endpoint: application.join("runtime/daemon.sock"),
                daemon,
                author: Mutex::new(SoftwareActorCustody::generate().expect("runtime author")),
                active_workspace: ActiveWorkspaceLink::new(&application),
                version_workspaces: VersionWorkspaceDirectory::new(&application),
                native_capture: NativeCapturePreference::new(&application),
                recent,
                recent_status: Mutex::new(RecentWorkspaceStatus::empty()),
                renderer_proof: RendererProofRuntime::disabled(),
                _server: Mutex::new(None),
            };

            let original_ran = std::cell::Cell::new(false);
            let refusal = with_verified_export_workspace(
                &runtime,
                original.to_str().expect("UTF-8 original"),
                &state.root,
                &state.digest,
                &state.installation,
                |targets_original| {
                    assert!(targets_original);
                    original_ran.set(true);
                    Ok(())
                },
            )
            .expect_err("unapproved private state must not update the original project");
            assert!(
                refusal.contains("exact current version approved as shared"),
                "{refusal}"
            );
            assert!(!original_ran.get());

            for overlap in [
                &workspace,
                &nested_original,
                &other_workspace,
                &other_project,
                &original_alias,
                &scratch,
            ] {
                let overlap_ran = std::cell::Cell::new(false);
                let refusal = with_verified_export_workspace(
                    &runtime,
                    overlap.to_str().expect("UTF-8 overlapping destination"),
                    &state.root,
                    &state.digest,
                    &state.installation,
                    |_| {
                        overlap_ran.set(true);
                        Ok(())
                    },
                )
                .expect_err("a private export must stay outside the original project");
                assert!(
                    refusal.contains("outside the original project"),
                    "{refusal}"
                );
                assert!(!overlap_ran.get());
            }

            let private_export_ran = std::cell::Cell::new(false);
            with_verified_export_workspace(
                &runtime,
                another_destination
                    .to_str()
                    .expect("UTF-8 private export destination"),
                &state.root,
                &state.digest,
                &state.installation,
                |targets_original| {
                    assert!(!targets_original);
                    private_export_ran.set(true);
                    Ok(())
                },
            )
            .expect("a separate explicit private export remains available");
            assert!(private_export_ran.get());

            let unknown_application = scratch.join("unknown-app");
            let unknown_workspace = scratch.join("unknown-workspace");
            let unknown_disjoint = scratch.join("unknown-disjoint");
            fs::create_dir(&unknown_application).expect("unknown app directory");
            fs::set_permissions(&unknown_application, fs::Permissions::from_mode(0o700))
                .expect("private unknown app directory");
            fs::create_dir(&unknown_workspace).expect("unknown workspace");
            fs::create_dir(&unknown_disjoint).expect("unknown disjoint destination");
            let unknown_recent = RecentWorkspace::new(unknown_application);
            unknown_recent
                .remember_repairing_invalid_content_at_point(
                    &unknown_workspace,
                    Some(&another_destination),
                    None,
                    None,
                )
                .expect("remember navigation without inventing project ancestry");
            unknown_recent
                .remember_with_navigation_hints(
                    &other_workspace,
                    Some(&other_project),
                    Some(&other_project),
                )
                .expect("remember a protected workspace beside the unknown-origin workspace");
            let unknown_entry = unknown_recent
                .load_entries()
                .expect("load unknown-origin navigation")
                .into_iter()
                .find(|entry| entry.project_root().is_none())
                .expect("unknown-origin navigation");
            assert_eq!(unknown_entry.project_root(), None);
            let overlap = export_targets_original_project(
                &unknown_recent,
                unknown_entry.path().to_str().expect("UTF-8 unknown root"),
                other_workspace
                    .to_str()
                    .expect("UTF-8 protected workspace destination"),
            )
            .expect_err("unknown ancestry must not bypass protected-tree overlap checks");
            assert!(
                overlap.contains("outside every managed workspace"),
                "{overlap}"
            );
            assert!(
                export_targets_original_project(
                    &unknown_recent,
                    unknown_entry.path().to_str().expect("UTF-8 unknown root"),
                    unknown_disjoint
                        .to_str()
                        .expect("UTF-8 unknown destination"),
                )
                .expect("classify unknown-origin export"),
                "unknown project ancestry must conservatively select the shared-version guard"
            );

            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn agent_handoff_is_durable_and_immediately_visible_to_the_browser() {
            let scratch = std::env::temp_dir()
                .join(format!("mesh-desktop-agent-handoff-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            let application = scratch.join("app");
            let workspace = scratch.join("workspace");
            fs::create_dir_all(&application).expect("app directory");
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private app directory");
            fs::create_dir(&workspace).expect("workspace");

            let recent = RecentWorkspace::new(application);
            let canonical = recent.remember(&workspace).expect("remember workspace");
            let status = Mutex::new(RecentWorkspaceStatus::empty());
            let handoff_generation = recent
                .record_agent_handoff(
                    &canonical,
                    "installation-agent-terminal",
                    &ProtectedWorkspaceRoot::inspect(&canonical)
                        .expect("workspace directory identity")
                        .directory_token(),
                )
                .expect("persist agent handoff");
            refresh_agent_handoff_navigation(&recent, &status)
                .expect("refresh agent handoff projection");

            let reloaded = recent.load_entries().expect("reload persisted history");
            assert_eq!(reloaded.len(), 1);
            assert_eq!(reloaded[0].path(), canonical);
            assert_eq!(
                reloaded[0].agent_handoff_installation(),
                Some("installation-agent-terminal")
            );
            let projected = status.lock().expect("status");
            assert_eq!(projected.workspace_entries.len(), 1);
            assert_eq!(
                projected.workspace_entries[0]
                    .agent_handoff_installation
                    .as_deref(),
                Some("installation-agent-terminal")
            );
            assert_eq!(
                projected.workspace_entries[0]
                    .agent_handoff_generation
                    .as_deref(),
                Some(handoff_generation.as_str())
            );
            drop(projected);
            assert!(recent
                .clear_agent_handoff(
                    &canonical,
                    "installation-agent-terminal",
                    &handoff_generation,
                )
                .expect("clear exact handoff"));
            refresh_agent_handoff_navigation(&recent, &status)
                .expect("refresh cleared handoff projection");
            assert_eq!(
                status.lock().expect("cleared status").workspace_entries[0]
                    .agent_handoff_installation
                    .as_deref(),
                None
            );
            let external = RecentWorkspace::new(recent.directory().to_path_buf());
            external
                .record_agent_handoff(
                    &canonical,
                    "installation-external-agent",
                    &ProtectedWorkspaceRoot::inspect(&canonical)
                        .expect("external workspace directory identity")
                        .directory_token(),
                )
                .expect("external process records handoff");
            assert_eq!(
                status.lock().expect("stale status").workspace_entries[0]
                    .agent_handoff_installation
                    .as_deref(),
                None,
                "the process-local projection changed without an explicit durable reload",
            );
            refresh_agent_handoff_navigation(&recent, &status)
                .expect("reload external handoff projection");
            assert_eq!(
                status.lock().expect("reloaded status").workspace_entries[0]
                    .agent_handoff_installation
                    .as_deref(),
                Some("installation-external-agent"),
            );

            let renamed_workspace = scratch.join("renamed-workspace");
            fs::rename(&canonical, &renamed_workspace).expect("rename assigned workspace");
            let renamed_canonical = external
                .remember(&renamed_workspace)
                .expect("remember renamed assigned workspace");
            refresh_agent_handoff_navigation(&recent, &status)
                .expect("project renamed agent handoff");
            let renamed_projection = status.lock().expect("renamed navigation status");
            assert_eq!(renamed_projection.workspace_entries.len(), 1);
            assert_eq!(
                renamed_projection.workspace_entries[0].path,
                renamed_canonical.display().to_string(),
            );
            assert_eq!(
                renamed_projection.workspace_entries[0]
                    .agent_handoff_installation
                    .as_deref(),
                Some("installation-external-agent"),
                "renaming the assigned directory hid its custody in the browser projection",
            );
            drop(renamed_projection);

            let other_workspace = scratch.join("other-workspace");
            fs::create_dir(&other_workspace).expect("other workspace");
            let other_canonical = external
                .remember(&other_workspace)
                .expect("external process remembers another workspace");
            refresh_agent_handoff_navigation(&recent, &status)
                .expect("reload external recent-workspace navigation");
            let reloaded_navigation = status.lock().expect("reloaded navigation status");
            assert_eq!(
                reloaded_navigation.remembered.as_deref(),
                other_canonical.to_str(),
                "Refresh kept the process-local remembered workspace after another process changed durable navigation",
            );
            assert_eq!(
                reloaded_navigation.workspaces,
                vec![
                    other_canonical.display().to_string(),
                    renamed_canonical.display().to_string(),
                ],
                "Refresh did not project the current bounded recent-workspace order",
            );
            assert_eq!(reloaded_navigation.workspace_entries.len(), 2);

            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn legacy_recent_custody_is_migrated_before_refresh_can_clear_it() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-agent-custody-migration-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let source = scratch.join("source");
            let managed = scratch.join("managed");
            let application = scratch.join("app");
            fs::create_dir_all(&source).expect("source");
            fs::write(source.join("work.txt"), b"agent work\n").expect("source file");
            fs::create_dir_all(&application).expect("app directory");
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700)).unwrap();
            let (confirmed, _) = mesh_daemon::PreparedFolderImport::prepare(&source, &managed)
                .expect("preview import")
                .confirm_into_workspace()
                .expect("confirm import");
            let presented = confirmed.destination().to_path_buf();
            let daemon = daemon();
            let summary = daemon
                .open_at_start(&presented)
                .expect("open managed workspace");
            let verified = daemon
                .verified_managed_workspace_path(
                    &summary.root,
                    &summary.digest,
                    &summary.installation,
                )
                .expect("verify native folder");
            let recent = RecentWorkspace::new(application);
            recent
                .remember(verified.path())
                .expect("remember workspace");
            let old_generation = recent
                .record_agent_handoff(
                    verified.path(),
                    &summary.installation,
                    &verified.directory_token(),
                )
                .expect("legacy Recent assignment");
            assert!(!daemon
                .workspace_agent_custody_for_workspace(
                    &summary.root,
                    &summary.digest,
                    &summary.installation,
                )
                .unwrap()
                .is_assigned());

            reconcile_current_agent_handoff(&daemon, &recent).expect("migrate legacy assignment");
            let shared = daemon
                .workspace_agent_custody_for_workspace(
                    &summary.root,
                    &summary.digest,
                    &summary.installation,
                )
                .expect("shared custody after migration");
            let generation = shared.generation().expect("assigned generation");
            assert_ne!(generation, old_generation);
            let entry = recent.load_entries().unwrap().remove(0);
            assert_eq!(entry.agent_handoff_generation(), Some(generation));
            assert!(
                confirmed.rollback().is_err(),
                "local rollback must refuse migrated custody"
            );

            daemon
                .release_workspace_agent_custody(
                    &summary.root,
                    &summary.digest,
                    &summary.installation,
                    generation,
                )
                .expect("release migrated custody");
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn active_agent_custody_refuses_working_copy_and_semantic_mutation_commands() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-managed-mutation-custody-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let application = scratch.join("app");
            let workspace = scratch.join("workspace.mesh");
            fs::create_dir_all(&application).expect("application data");
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private application data");
            fs::create_dir(&workspace).expect("workspace");
            let recent = RecentWorkspace::new(application);
            let admitted = recent.remember(&workspace).expect("remember workspace");
            let installation = "installation-managed-mutation-custody";
            recent
                .record_agent_handoff(
                    &admitted,
                    installation,
                    &ProtectedWorkspaceRoot::inspect(&admitted)
                        .expect("workspace directory identity")
                        .directory_token(),
                )
                .expect("active agent custody");

            for command in [
                "preserve_managed_text",
                "create_managed_text",
                "restore_managed_version",
                "rollback_managed_workspace",
                "open_current_review",
                "approve_current_review",
            ] {
                let entered = std::cell::Cell::new(false);
                let refusal = with_unassigned_managed_workspace(
                    &recent,
                    admitted.to_str().expect("UTF-8 workspace"),
                    installation,
                    || {
                        entered.set(true);
                        Ok(())
                    },
                )
                .expect_err("active custody must refuse the representative managed mutation");
                assert!(!entered.get(), "{command} entered its mutation closure");
                assert!(
                    refusal.contains("assigned to an agent"),
                    "{command}: {refusal}"
                );
            }

            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn generic_browser_daemon_bridge_cannot_bypass_verified_mutation_commands() {
            for method in ["workspace.state", "folder.import.preview"] {
                assert!(desktop_daemon_call_is_read_only(method), "{method}");
            }
            for method in [
                "folder.import.confirm",
                "folder.import.rollback",
                "review.open",
                "review.approve",
            ] {
                assert!(!desktop_daemon_call_is_read_only(method), "{method}");
            }
        }

        #[test]
        fn full_agent_custody_refuses_import_and_version_creation_before_native_mutation() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-agent-custody-capacity-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let application = scratch.join("app");
            let source = scratch.join("ordinary-source");
            fs::create_dir_all(&application).expect("application data");
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private application data");
            fs::create_dir(&source).expect("ordinary source");
            fs::write(source.join("notes.txt"), "ordinary bytes\n").expect("source file");

            let recent = RecentWorkspace::new(application.clone());
            for index in 0..8 {
                let workspace = scratch.join(format!("assigned-{index}"));
                fs::create_dir(&workspace).expect("assigned workspace");
                let canonical = recent.remember(&workspace).expect("remember assigned");
                let directory = ProtectedWorkspaceRoot::inspect(&canonical)
                    .expect("assigned directory identity")
                    .directory_token();
                recent
                    .record_agent_handoff(&canonical, &format!("installation-{index}"), &directory)
                    .expect("record active handoff");
            }
            let record_before = fs::read(application.join("recent-workspace.json"))
                .expect("custody record before refusal");
            let daemon = Arc::new(daemon());
            let preview = daemon
                .preview_folder_import(&source.display().to_string())
                .expect("preview ordinary source");
            let summary = preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("preview summary")
                .to_owned();
            let runtime = DesktopRuntime {
                endpoint: application.join("runtime/daemon.sock"),
                daemon,
                author: Mutex::new(SoftwareActorCustody::generate().expect("local author")),
                active_workspace: ActiveWorkspaceLink::new(&application),
                version_workspaces: VersionWorkspaceDirectory::new(&application),
                native_capture: NativeCapturePreference::new(&application),
                recent,
                recent_status: Mutex::new(RecentWorkspaceStatus::empty()),
                renderer_proof: RendererProofRuntime::disabled(),
                _server: Mutex::new(None),
            };

            let import_error =
                import_managed_workspace_for(&runtime, source.display().to_string(), summary, None)
                    .expect_err("import must refuse before creating a ninth workspace");
            assert!(
                import_error.contains("active agent folders"),
                "{import_error}"
            );
            let version_error = open_workspace_version_copy(
                &runtime.daemon,
                &runtime.version_workspaces,
                &"11".repeat(32),
                None,
                None,
                WorkspaceVersionSource {
                    root: "/unreached",
                    digest: "unreached",
                    installation: "unreached",
                    recent: &runtime.recent,
                },
                false,
            )
            .expect_err("version creation must refuse before allocating a ninth workspace");
            assert!(
                version_error.contains("active agent folders"),
                "{version_error}"
            );
            assert_eq!(
                fs::read(application.join("recent-workspace.json"))
                    .expect("custody record after refusal"),
                record_before
            );
            assert!(
                !application.join("workspace-versions").exists(),
                "a refused operation allocated private workspace storage"
            );
            assert!(runtime.daemon.workspace_state().is_err());
            assert_eq!(
                fs::read_to_string(source.join("notes.txt")).unwrap(),
                "ordinary bytes\n"
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn custom_import_destination_cannot_enter_a_remembered_workspace() {
            let scratch = std::env::temp_dir().join(format!("mesh-dci-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            let application = scratch.join("app");
            let runtime_directory = application.join("runtime");
            let first_source = scratch.join("first-source");
            let second_source = scratch.join("second-source");
            let first_store = scratch.join("first.mesh");
            for directory in [&runtime_directory, &first_source, &second_source] {
                fs::create_dir_all(directory).expect("fixture directory");
            }
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private application data");
            fs::write(first_source.join("first.txt"), "first workspace\n")
                .expect("first source file");
            fs::write(second_source.join("second.txt"), "second workspace\n")
                .expect("second source file");

            let daemon = Arc::new(daemon());
            let first_preview = daemon
                .preview_folder_import(&first_source.display().to_string())
                .expect("preview first import");
            let first_summary = first_preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("first summary");
            daemon
                .confirm_folder_import(
                    &first_source.display().to_string(),
                    &first_store.display().to_string(),
                    first_summary,
                )
                .expect("confirm first import");
            let first_state = daemon.workspace_state().expect("first workspace state");
            let first_root = PathBuf::from(&first_state.root);

            let recent = RecentWorkspace::new(application.clone());
            recent
                .remember_with_navigation_hints(
                    &first_root,
                    Some(&first_source),
                    Some(&first_source),
                )
                .expect("remember first workspace");
            let endpoint = runtime_directory.join("daemon.sock");
            let operations: Arc<dyn Operations> = daemon.clone();
            let server = IpcServer::bind(&endpoint)
                .expect("bind app daemon")
                .spawn(operations)
                .expect("serve app daemon");
            let runtime = DesktopRuntime {
                endpoint,
                daemon,
                author: Mutex::new(SoftwareActorCustody::generate().expect("local author")),
                active_workspace: ActiveWorkspaceLink::new(&application),
                version_workspaces: VersionWorkspaceDirectory::new(&application),
                native_capture: NativeCapturePreference::new(&application),
                recent,
                recent_status: Mutex::new(RecentWorkspaceStatus::empty()),
                renderer_proof: RendererProofRuntime::disabled(),
                _server: Mutex::new(Some(server)),
            };

            let second_preview = runtime
                .daemon
                .preview_folder_import(&second_source.display().to_string())
                .expect("preview second import");
            let second_summary = second_preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("second summary")
                .to_owned();
            let nested = first_root.join("nested-second.mesh");
            let error = import_managed_workspace_for(
                &runtime,
                second_source.display().to_string(),
                second_summary.clone(),
                Some(nested.display().to_string()),
            )
            .expect_err("a custom import must stay outside every remembered workspace");

            assert!(error.contains("outside every workspace"), "{error}");
            assert!(!nested.exists(), "the refused nested workspace was created");
            assert_eq!(
                runtime
                    .daemon
                    .workspace_state()
                    .expect("current workspace")
                    .root,
                first_state.root,
                "the refused import replaced the current workspace"
            );
            assert_eq!(
                fs::read_to_string(first_root.join("first.txt")).unwrap(),
                "first workspace\n"
            );

            // The native preflight can race another same-user process. Carrying exact directory
            // identities through IPC lets the daemon catch a formerly safe destination parent
            // that was replaced by a protected workspace before create-only allocation.
            let safe_parent = scratch.join("safe-parent");
            let displaced_safe_parent = scratch.join("displaced-safe-parent");
            let protected_workspace = scratch.join("protected-workspace");
            fs::create_dir(&safe_parent).expect("safe destination parent");
            fs::create_dir(&protected_workspace).expect("protected workspace");
            fs::write(protected_workspace.join("agent.txt"), "active agent work\n")
                .expect("protected work");
            let protected = ProtectedWorkspaceRoot::inspect(&protected_workspace)
                .expect("protected directory identity");
            fs::rename(&safe_parent, &displaced_safe_parent).expect("displace safe parent");
            fs::rename(&protected_workspace, &safe_parent)
                .expect("redirect destination into protected workspace");
            let redirected = safe_parent.join("redirected.mesh");
            let error = call_daemon(
                &runtime.endpoint,
                "folder.import.confirm".to_owned(),
                Json::object([
                    ("source", Json::text(second_source.to_string_lossy())),
                    ("destination", Json::text(redirected.to_string_lossy())),
                    ("summary", Json::text(&second_summary)),
                    (
                        "protected_roots",
                        Json::Array(vec![Json::text(protected.directory_token())]),
                    ),
                ])
                .encode(),
            )
            .expect_err("the daemon must reject a protected parent swapped in after preflight");
            assert!(error.contains("outside every workspace"), "{error}");
            assert!(!redirected.exists(), "the redirected workspace was created");
            assert_eq!(
                fs::read_to_string(safe_parent.join("agent.txt")).unwrap(),
                "active agent work\n"
            );
            drop(runtime);
            fs::remove_dir_all(scratch).expect("cleanup");
        }

        #[test]
        fn desktop_existing_workspace_open_never_initializes_an_ordinary_project() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-existing-only-open-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let project = scratch.join("ordinary-project");
            fs::create_dir_all(&project).expect("ordinary project");
            fs::write(project.join("user.txt"), b"user bytes\n").expect("ordinary file");
            let params = Json::object([("path", Json::text(project.to_string_lossy()))]).encode();

            let current = scratch.join("current-managed");
            let daemon = daemon();
            daemon
                .open_workspace(current.to_string_lossy().as_ref())
                .expect("open current managed workspace");
            let before_workspace = daemon.workspace_state().expect("current workspace");
            let before_startup = daemon.startup();

            let refusal = reopen_existing_managed_workspace(&daemon, &params)
                .expect_err("ordinary project is not existing Mesh history");

            let refusal = Json::parse(&refusal).expect("structured desktop refusal");
            assert_eq!(
                refusal.get("kind").and_then(Json::as_text),
                Some(DAEMON_REFUSAL_KIND)
            );
            assert_eq!(
                refusal.get("code").and_then(Json::as_text),
                Some("workspace-unreachable")
            );
            assert!(refusal
                .get("message")
                .and_then(Json::as_text)
                .is_some_and(|message| message.contains("Choose Import")));
            assert_eq!(
                fs::read(project.join("user.txt")).expect("unchanged ordinary file"),
                b"user bytes\n"
            );
            assert!(
                !project
                    .join(mesh_daemon::workspace::STORAGE_DIRECTORY_NAME)
                    .exists(),
                "existing-only desktop open must not initialize private state"
            );
            assert_eq!(
                fs::read_dir(&project).expect("ordinary project").count(),
                1,
                "the refusal must add no file or directory"
            );
            assert_eq!(
                daemon
                    .workspace_state()
                    .expect("workspace remains open")
                    .root,
                before_workspace.root,
                "a refused existing-only open must preserve the current workspace"
            );
            assert_eq!(
                daemon.startup(),
                before_startup,
                "a runtime refusal must not replace the process start-up report"
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn desktop_existing_workspace_open_reopens_an_initialized_empty_workspace() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-existing-only-reopen-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let managed = scratch.join("managed");
            let initialized = mesh_daemon::workspace::OpenWorkspace::open(&managed)
                .expect("initialize empty managed workspace");
            let expected_root = initialized.root().as_path().display().to_string();
            drop(initialized);
            let params = Json::object([("path", Json::text(&expected_root))]).encode();

            let answer = reopen_existing_managed_workspace(&daemon(), &params)
                .expect("reopen initialized workspace");
            let answer = Json::parse(&answer).expect("workspace answer");
            assert_eq!(
                answer.get("root").and_then(Json::as_text),
                Some(expected_root.as_str())
            );
            assert_eq!(answer.get("records").and_then(Json::as_u64), Some(0));
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn opened_version_response_carries_the_navigation_already_committed_by_native() {
            let navigation = RecentWorkspaceStatus {
                remembered: Some("/managed/earlier".to_owned()),
                workspaces: vec!["/managed/earlier".to_owned(), "/managed/current".to_owned()],
                workspace_entries: vec![
                    RecentWorkspaceNavigationEntry {
                        path: "/managed/earlier".to_owned(),
                        export_root: Some("/ordinary/original".to_owned()),
                        project_root: Some("/ordinary/original".to_owned()),
                        agent_handoff_installation: None,
                        agent_handoff_generation: None,
                        source_point_ordinal: Some(1),
                        original_update_version: Some("shared-earlier".to_owned()),
                    },
                    RecentWorkspaceNavigationEntry {
                        path: "/managed/current".to_owned(),
                        export_root: Some("/ordinary/original".to_owned()),
                        project_root: Some("/ordinary/original".to_owned()),
                        agent_handoff_installation: None,
                        agent_handoff_generation: None,
                        source_point_ordinal: None,
                        original_update_version: Some("shared-earlier".to_owned()),
                    },
                ],
                auto_opened: false,
                active_folder: Some("/application/native-workspace/current".to_owned()),
                export_root: Some("/ordinary/original".to_owned()),
                warning: None,
            };
            let answer = Json::object([
                ("source_version", Json::text("saved-version")),
                ("destination", Json::text("/managed/earlier")),
                (
                    "workspace",
                    Json::object([("root", Json::text("/managed/earlier"))]),
                ),
            ])
            .encode();

            let encoded = encode_opened_workspace_version_response(&answer, &navigation)
                .expect("attach committed navigation");
            let response = Json::parse(&encoded).expect("response JSON");
            assert_eq!(
                response.get("destination").and_then(Json::as_text),
                Some("/managed/earlier")
            );
            let committed = response.get("navigation").expect("navigation receipt");
            assert_eq!(
                committed.get("remembered").and_then(Json::as_text),
                Some("/managed/earlier")
            );
            assert_eq!(
                committed.get("active_folder").and_then(Json::as_text),
                Some("/application/native-workspace/current")
            );
            assert_eq!(
                committed.get("export_root").and_then(Json::as_text),
                Some("/ordinary/original")
            );
        }

        #[test]
        fn native_import_defaults_to_app_managed_storage_and_commits_navigation() {
            let scratch = std::env::temp_dir()
                .join(format!("mesh-desktop-app-import-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            let application = scratch.join("app");
            let runtime_directory = application.join("runtime");
            let source = scratch.join("ordinary-source");
            fs::create_dir_all(&runtime_directory).expect("private app runtime");
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private app data");
            fs::create_dir(&source).expect("ordinary source");
            fs::write(
                source.join("notes.txt"),
                "ordinary source stays unchanged\n",
            )
            .expect("source file");

            let daemon = Arc::new(daemon());
            let preview = daemon
                .preview_folder_import(&source.display().to_string())
                .expect("preview import");
            let summary = preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("preview summary")
                .to_owned();
            let endpoint = runtime_directory.join("daemon.sock");
            let operations: Arc<dyn Operations> = daemon.clone();
            let server = IpcServer::bind(&endpoint)
                .expect("bind app daemon")
                .spawn(operations)
                .expect("serve app daemon");
            let runtime = DesktopRuntime {
                endpoint,
                daemon,
                author: Mutex::new(SoftwareActorCustody::generate().expect("local author")),
                active_workspace: ActiveWorkspaceLink::new(&application),
                version_workspaces: VersionWorkspaceDirectory::new(&application),
                native_capture: NativeCapturePreference::new(&application),
                recent: RecentWorkspace::new(application.clone()),
                recent_status: Mutex::new(RecentWorkspaceStatus::empty()),
                renderer_proof: RendererProofRuntime::disabled(),
                _server: Mutex::new(Some(server)),
            };

            let encoded = import_managed_workspace_for(
                &runtime,
                source.display().to_string(),
                summary.clone(),
                None,
            )
            .expect("app-managed import");
            let answer = Json::parse(&encoded).expect("native import response");
            let workspace = answer.get("workspace").expect("opened workspace");
            let root = PathBuf::from(
                workspace
                    .get("root")
                    .and_then(Json::as_text)
                    .expect("workspace root"),
            );
            let private_store = root.parent().expect("presented mounts parent");
            assert_eq!(
                private_store.parent(),
                Some(application.join("workspace-versions").as_path())
            );
            assert_eq!(
                private_store.file_name().and_then(|name| name.to_str()),
                Some(format!("workspace-{}.mesh", &summary[..12]).as_str())
            );
            assert_eq!(
                fs::read_to_string(root.join("notes.txt")).unwrap(),
                "ordinary source stays unchanged\n"
            );
            assert_eq!(
                fs::read_to_string(source.join("notes.txt")).unwrap(),
                "ordinary source stays unchanged\n"
            );

            let navigation = answer.get("navigation").expect("committed navigation");
            let stable = PathBuf::from(
                navigation
                    .get("active_folder")
                    .and_then(Json::as_text)
                    .expect("stable working folder"),
            );
            assert_eq!(
                fs::read_link(&stable).unwrap(),
                fs::canonicalize(&root).unwrap()
            );
            assert_eq!(
                navigation.get("export_root").and_then(Json::as_text),
                fs::canonicalize(&source).unwrap().to_str()
            );
            let remembered = runtime.recent.load_entries().expect("recent workspaces");
            assert_eq!(remembered.len(), 1);
            assert_eq!(remembered[0].path(), fs::canonicalize(&root).unwrap());
            assert_eq!(
                remembered[0].export_root(),
                Some(fs::canonicalize(&source).unwrap().as_path())
            );

            fs::remove_file(application.join("native-workspace/current"))
                .expect("simulate crash before stable link publication");
            fs::remove_file(application.join("recent-workspace.json"))
                .expect("simulate crash before recent workspace publication");
            *runtime.recent_status.lock().unwrap() = RecentWorkspaceStatus::empty();
            let recovered = Json::parse(
                &import_managed_workspace_for(
                    &runtime,
                    source.display().to_string(),
                    summary.clone(),
                    None,
                )
                .expect("recover confirmed import"),
            )
            .expect("recovered response");
            assert_eq!(
                recovered
                    .get("recovered_after_interruption")
                    .and_then(Json::as_bool),
                Some(true)
            );
            let recovered_root = PathBuf::from(
                recovered
                    .get("workspace")
                    .and_then(|workspace| workspace.get("root"))
                    .and_then(Json::as_text)
                    .expect("recovered workspace root"),
            );
            assert_eq!(recovered_root, root);
            assert!(
                !application
                    .join("workspace-versions")
                    .join(format!("workspace-{}-2.mesh", &summary[..12]))
                    .exists(),
                "recovery must not allocate a duplicate workspace"
            );

            let unrelated = scratch.join("same-content-unrelated-source");
            fs::create_dir(&unrelated).expect("unrelated source");
            fs::write(
                unrelated.join("notes.txt"),
                "ordinary source stays unchanged\n",
            )
            .expect("same bytes at another source");
            let distinct = Json::parse(
                &import_managed_workspace_for(
                    &runtime,
                    unrelated.display().to_string(),
                    summary.clone(),
                    None,
                )
                .expect("import unrelated same-content source"),
            )
            .expect("distinct response");
            assert_eq!(
                distinct.get("recovered_after_interruption"),
                None,
                "matching bytes alone must not grant recovery authority"
            );
            let distinct_root = PathBuf::from(
                distinct
                    .get("workspace")
                    .and_then(|workspace| workspace.get("root"))
                    .and_then(Json::as_text)
                    .expect("distinct workspace root"),
            );
            let distinct_name = format!("workspace-{}-2.mesh", &summary[..12]);
            assert_eq!(
                distinct_root.parent().and_then(|path| path.file_name()),
                Some(std::ffi::OsStr::new(&distinct_name))
            );

            drop(runtime);
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn desktop_adopts_the_exact_current_cli_import_without_copying_it_again() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-current-cli-import-recovery-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let application = scratch.join("app");
            let source = scratch.join("ordinary-source");
            let cli_managed = scratch.join("cli-managed");
            fs::create_dir_all(&application).expect("application data");
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private application data");
            fs::create_dir(&source).expect("ordinary source");
            fs::write(source.join("notes.txt"), "agent-ready ordinary bytes\n")
                .expect("source file");

            let daemon = Arc::new(daemon());
            let preview = daemon
                .preview_folder_import(&source.display().to_string())
                .expect("preview ordinary source");
            let summary = preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("preview summary")
                .to_owned();
            let cli_answer = daemon
                .confirm_folder_import(
                    &source.display().to_string(),
                    &cli_managed.display().to_string(),
                    &summary,
                )
                .expect("live CLI import");
            let cli_root = PathBuf::from(
                cli_answer
                    .get("workspace")
                    .and_then(|workspace| workspace.get("root"))
                    .and_then(Json::as_text)
                    .expect("live CLI workspace root"),
            );
            assert_eq!(
                cli_root,
                cli_managed.join(mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME)
            );
            let checkpoint = Json::parse(
                &managed_checkpoint_state_for(&daemon).expect("current checkpoint state"),
            )
            .expect("checkpoint JSON");
            assert_eq!(
                checkpoint
                    .get("confirmed_import_receipt")
                    .and_then(Json::as_bool),
                Some(true),
                "the UI needs a read-only hint until the original-project binding is restored"
            );

            let runtime = DesktopRuntime {
                endpoint: application.join("runtime/daemon.sock"),
                daemon,
                author: Mutex::new(SoftwareActorCustody::generate().expect("local author")),
                active_workspace: ActiveWorkspaceLink::new(&application),
                version_workspaces: VersionWorkspaceDirectory::new(&application),
                native_capture: NativeCapturePreference::new(&application),
                recent: RecentWorkspace::new(application.clone()),
                recent_status: Mutex::new(RecentWorkspaceStatus::empty()),
                renderer_proof: RendererProofRuntime::disabled(),
                _server: Mutex::new(None),
            };

            let recovered = Json::parse(
                &import_managed_workspace_for(
                    &runtime,
                    source.display().to_string(),
                    summary,
                    None,
                )
                .expect("adopt exact live import"),
            )
            .expect("adoption response");
            assert_eq!(
                recovered
                    .get("recovered_after_interruption")
                    .and_then(Json::as_bool),
                Some(true)
            );
            assert_eq!(
                recovered
                    .get("workspace")
                    .and_then(|workspace| workspace.get("root"))
                    .and_then(Json::as_text),
                cli_root.to_str()
            );
            assert!(
                !application.join("workspace-versions").exists(),
                "adopting the exact live import must not allocate a duplicate workspace"
            );

            let navigation = recovered.get("navigation").expect("committed navigation");
            let stable = PathBuf::from(
                navigation
                    .get("active_folder")
                    .and_then(Json::as_text)
                    .expect("stable working folder"),
            );
            assert_eq!(
                fs::read_link(&stable).unwrap(),
                fs::canonicalize(&cli_root).unwrap()
            );
            assert_eq!(
                navigation.get("export_root").and_then(Json::as_text),
                fs::canonicalize(&source).unwrap().to_str()
            );
            let remembered = runtime.recent.load_entries().expect("recent workspace");
            assert_eq!(remembered.len(), 1);
            assert_eq!(remembered[0].path(), fs::canonicalize(&cli_root).unwrap());
            assert_eq!(
                remembered[0].project_root(),
                Some(fs::canonicalize(&source).unwrap().as_path())
            );
            assert_eq!(
                fs::read_to_string(source.join("notes.txt")).unwrap(),
                "agent-ready ordinary bytes\n"
            );

            drop(runtime);
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn native_folder_opening_has_one_fixed_program_and_one_path_argument() {
            assert!(std::path::Path::new(NATIVE_FOLDER_OPENER).is_absolute());
            assert!(!NATIVE_FOLDER_OPENER.contains(' '));
        }

        #[test]
        fn exact_copy_command_compares_closed_artifact_families_not_raster_suffixes() {
            assert!(review_inspection_families_match(["png", "jpg"]));
            assert!(review_inspection_families_match(["gif", "webp"]));
            assert!(review_inspection_families_match(["pdf", "pdf"]));
            assert!(!review_inspection_families_match(["pdf", "png"]));
            assert!(!review_inspection_families_match(["docx", "pptx"]));
            assert!(!review_inspection_families_match(["bin"]));
        }

        #[test]
        fn saved_side_identity_and_content_type_are_admitted_before_materialization() {
            let version = "11".repeat(32);
            let digest = "22".repeat(32);
            assert_eq!(
                admitted_review_inspection_extension(
                    "empty.txt",
                    b"",
                    &version,
                    &digest,
                    &version,
                    &digest,
                    "open-entry",
                )
                .unwrap(),
                "txt"
            );
            assert_eq!(
                admitted_review_inspection_extension(
                    ".DS_Store",
                    b"\0opaque\xffbytes",
                    &version,
                    &digest,
                    &version,
                    &digest,
                    "reveal-entry",
                )
                .unwrap(),
                "bin"
            );
            assert!(admitted_review_inspection_extension(
                ".DS_Store",
                b"\0opaque\xffbytes",
                &version,
                &digest,
                &version,
                &digest,
                "open-entry",
            )
            .unwrap_err()
            .contains("Reveal in Finder"));
            assert_eq!(
                admitted_review_inspection_extension(
                    "assets/hero.png",
                    b"\x89PNG\r\n\x1a\nexact saved bytes",
                    &version,
                    &digest,
                    &version,
                    &digest,
                    "open-entry",
                )
                .unwrap(),
                "png"
            );
            assert!(admitted_review_inspection_extension(
                "assets/spoofed.png",
                b"not an image\xff",
                &version,
                &digest,
                &version,
                &digest,
                "open-entry",
            )
            .is_err());
            assert!(admitted_review_inspection_extension(
                "empty.txt",
                b"",
                &version,
                &digest,
                &"33".repeat(32),
                &digest,
                "reveal-entry",
            )
            .is_err());
            assert!(admitted_review_inspection_extension(
                "empty.txt",
                b"",
                &version,
                &digest,
                &version,
                &"44".repeat(32),
                "open-folder",
            )
            .is_err());
        }

        #[test]
        fn native_folder_opening_reports_the_opener_process_result() {
            let folder = std::path::Path::new("/folder handed to the opener as one argument");
            open_native_folder_with(
                std::path::Path::new("/usr/bin/true"),
                folder,
                Duration::from_secs(1),
            )
            .expect("successful opener");

            let refusal = open_native_folder_with(
                std::path::Path::new("/usr/bin/false"),
                folder,
                Duration::from_secs(1),
            )
            .expect_err("nonzero opener result must not be reported as success");
            assert!(refusal.contains("could not open the working folder"));

            let unavailable = open_native_folder_with(
                std::path::Path::new("/definitely/missing/mesh-folder-opener"),
                folder,
                Duration::from_secs(1),
            )
            .expect_err("missing opener must not be reported as success");
            assert!(unavailable.contains("opener could not start"));

            let timeout = open_native_folder_with(
                std::path::Path::new("/bin/sleep"),
                std::path::Path::new("1"),
                Duration::from_millis(10),
            )
            .expect_err("a stuck opener must not block the desktop indefinitely");
            assert!(timeout.contains("did not finish"));
        }

        #[test]
        fn workspace_entry_launcher_uses_only_closed_argument_shapes() {
            let scratch = std::env::temp_dir()
                .join(format!("mesh-desktop-entry-argv-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch directory");
            let launcher = scratch.join("capture-entry-argv.sh");
            let captured = scratch.join("argv.txt");
            fs::write(
                &launcher,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                    captured.display()
                ),
            )
            .expect("capture launcher");
            fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700))
                .expect("executable capture launcher");
            let entry = scratch.join("report with spaces.txt");
            fs::write(&entry, b"report\n").expect("entry");

            open_native_entry_with(
                &launcher,
                &entry,
                "open-entry",
                None,
                Duration::from_secs(1),
            )
            .expect("default application launch");
            assert_eq!(
                fs::read_to_string(&captured).expect("captured open arguments"),
                format!("{}\n", entry.display()),
            );
            #[cfg(target_os = "macos")]
            {
                let application = Path::new("/System/Applications/Preview.app");
                open_native_entry_with(
                    &launcher,
                    &entry,
                    "open-entry",
                    Some(application),
                    Duration::from_secs(1),
                )
                .expect("resolved default application launch");
                assert_eq!(
                    fs::read_to_string(&captured).expect("captured application arguments"),
                    format!("-a\n{}\n{}\n", application.display(), entry.display()),
                );
                let directory = scratch.join("folder with spaces");
                fs::create_dir(&directory).expect("directory entry");
                open_native_entry_with(
                    &launcher,
                    &directory,
                    "open-entry",
                    Some(Path::new(FINDER_APPLICATION_PATH)),
                    Duration::from_secs(1),
                )
                .expect("explicit Finder directory launch");
                assert_eq!(
                    fs::read_to_string(&captured).expect("captured Finder arguments"),
                    format!("-a\n{}\n{}\n", FINDER_APPLICATION_PATH, directory.display()),
                );
            }
            open_native_entry_with(
                &launcher,
                &entry,
                "reveal-entry",
                None,
                Duration::from_secs(1),
            )
            .expect("Finder reveal launch");
            #[cfg(target_os = "macos")]
            assert_eq!(
                fs::read_to_string(&captured).expect("captured reveal arguments"),
                format!("-R\n{}\n", entry.display()),
            );
            assert!(open_native_entry_with(
                &launcher,
                &entry,
                "arbitrary-action",
                None,
                Duration::from_secs(1),
            )
            .is_err());
            fs::remove_dir_all(scratch).unwrap();
        }

        #[test]
        fn codex_workspace_launch_uses_explicit_session_configuration() {
            let scratch = std::env::temp_dir()
                .join(format!("mesh-desktop-codex-argv-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch directory");
            let launcher = scratch.join("capture-codex-argv.sh");
            let captured = scratch.join("argv.txt");
            fs::write(
                &launcher,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                    captured.display()
                ),
            )
            .expect("capture launcher");
            fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700))
                .expect("executable capture launcher");
            let folder = scratch.join("workspace handed to Codex as one argument");
            fs::create_dir_all(&folder).expect("workspace directory");
            let overrides = vec![
                "mcp_servers.mesh.command=\"/Applications/Mesh.app/Contents/MacOS/Mesh\""
                    .to_owned(),
                "mcp_servers.mesh.enabled_tools=[\"mesh_workspace_state\"]".to_owned(),
            ];
            open_codex_workspace_with(&launcher, &folder, &overrides, Duration::from_secs(1))
                .expect("successful Codex launcher");
            assert_eq!(
                fs::read_to_string(&captured).expect("captured arguments"),
                format!(
                    "app\n-c\n{}\n-c\n{}\n{}\n",
                    overrides[0],
                    overrides[1],
                    folder.display()
                ),
                "Codex must receive every launch-scoped override before one exact workspace path"
            );

            let refusal = open_codex_workspace_with(
                std::path::Path::new("/usr/bin/false"),
                &folder,
                &overrides,
                Duration::from_secs(1),
            )
            .expect_err("nonzero Codex launcher result must not be reported as success");
            assert!(refusal.contains("Codex could not open"));
            fs::remove_dir_all(&scratch).expect("clean scratch directory");
        }

        #[test]
        #[cfg(target_os = "macos")]
        fn codex_launcher_receives_the_admitted_directory_after_namespace_replacement() {
            use mesh_mcp::WorkspaceStateProvider as _;

            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-codex-reference-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch directory");
            fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))
                .expect("private scratch permissions");
            let scratch = fs::canonicalize(&scratch).expect("canonical scratch directory");
            let workspace = scratch.join("workspace");
            let displaced = scratch.join("displaced");
            fs::create_dir(&workspace).expect("workspace");
            fs::write(workspace.join("admitted.txt"), "admitted\n").expect("admitted marker");
            let daemon = Arc::new(daemon());
            let state = daemon
                .open_workspace(&workspace.display().to_string())
                .expect("open workspace");
            let verified = daemon
                .verified_managed_workspace_path(&state.root, &state.digest, &state.installation)
                .expect("verified workspace");
            let reference = verified.stable_agent_reference().expect("stable reference");
            let endpoint = scratch.join("daemon.sock");
            let operations: Arc<dyn Operations> = daemon.clone();
            let server = IpcServer::bind(&endpoint)
                .expect("bind daemon")
                .spawn(operations)
                .expect("serve daemon");
            let executable = std::env::current_exe().expect("test executable");
            let overrides = codex_launch_overrides(
                &executable,
                &endpoint,
                verified.path(),
                &state.installation,
            )
            .expect("logical-root overrides");
            assert!(
                overrides
                    .iter()
                    .any(|override_value| override_value.contains(&state.root)),
                "the MCP binding must carry the daemon's canonical logical root"
            );
            assert!(
                overrides.iter().all(
                    |override_value| !override_value.contains(&reference.display().to_string())
                ),
                "the filesystem reference must not replace the MCP logical identity"
            );
            mesh_mcp::DaemonWorkspaceState::bound(
                endpoint.clone(),
                mesh_mcp::ExpectedWorkspace::new(
                    PathBuf::from(&state.root),
                    state.installation.clone(),
                ),
            )
            .workspace_state()
            .expect("the MCP layer accepts the daemon's exact workspace state");

            fs::rename(&workspace, &displaced).expect("displace admitted workspace");
            fs::create_dir(&workspace).expect("replacement workspace");
            fs::write(workspace.join("replacement.txt"), "replacement\n")
                .expect("replacement marker");
            let launcher = scratch.join("verify-codex-reference.sh");
            fs::write(
                &launcher,
                "#!/bin/sh\nlast=''\nfor value in \"$@\"; do last=\"$value\"; done\ntest -f \"$last/admitted.txt\" || exit 41\ntest ! -e \"$last/replacement.txt\" || exit 42\n",
            )
            .expect("launcher fixture");
            fs::set_permissions(&launcher, fs::Permissions::from_mode(0o700))
                .expect("launcher permissions");

            open_codex_workspace_with(&launcher, &reference, &overrides, Duration::from_secs(5))
                .expect("launcher resolves the admitted directory identity");
            assert!(verified.ensure_current().is_err());
            drop(server);
            let _ = fs::remove_file(&endpoint);
            let _ = fs::remove_file(endpoint.with_extension("sock.lock"));
            fs::remove_dir_all(&workspace).expect("remove replacement");
            fs::remove_dir_all(&displaced).expect("remove admitted workspace");
            fs::remove_file(&launcher).expect("remove launcher");
            fs::remove_dir(&scratch).expect("remove scratch");
        }

        #[test]
        fn optional_mesh_context_never_blocks_a_healthy_codex_workspace() {
            let folder = std::path::Path::new("/workspace with user-owned Codex settings");
            let launched = std::cell::Cell::new(false);
            let outcome = open_codex_after_optional_context(
                folder,
                Err(CodexProjectConfigError::ExistingConfig(
                    folder.join(".codex/config.toml"),
                )),
                Ok(vec!["mcp_servers.mesh.required=false".to_owned()]),
                |opened, overrides| {
                    assert_eq!(opened, folder);
                    assert_eq!(overrides, ["mcp_servers.mesh.required=false"]);
                    launched.set(true);
                    Ok(true)
                },
            )
            .expect("optional context refusal must not block Codex");
            assert!(launched.get());
            assert_eq!(outcome.state, "ready");
            assert_eq!(outcome.warning, None);

            let launch_failure = open_codex_after_optional_context(
                folder,
                Ok(CodexProjectConfig::Current),
                Ok(vec!["mcp_servers.mesh.required=false".to_owned()]),
                |_, _| Err("launcher refused".to_owned()),
            )
            .expect_err("an actual launcher failure must remain visible");
            assert_eq!(launch_failure, "launcher refused");

            let fallback = open_codex_after_optional_context(
                folder,
                Ok(CodexProjectConfig::Current),
                Ok(vec!["mcp_servers.mesh.required=false".to_owned()]),
                |_, _| Ok(false),
            )
            .expect("Codex remains usable without the CLI bridge");
            assert_eq!(fallback.state, "unavailable");
            assert!(fallback
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("launch-scoped context")));
        }

        #[test]
        fn terminal_workspace_launch_is_one_fixed_application_and_one_workspace_argument() {
            assert_eq!(TERMINAL_BUNDLE_IDENTIFIER, "com.apple.Terminal");
            let folder = std::path::Path::new("/workspace handed to Terminal as one argument");
            open_terminal_workspace_with(
                std::path::Path::new("/usr/bin/true"),
                TERMINAL_BUNDLE_IDENTIFIER,
                folder,
                Duration::from_secs(1),
            )
            .expect("successful Terminal launcher");

            let refusal = open_terminal_workspace_with(
                std::path::Path::new("/usr/bin/false"),
                TERMINAL_BUNDLE_IDENTIFIER,
                folder,
                Duration::from_secs(1),
            )
            .expect_err("nonzero Terminal launcher result must not be reported as success");
            assert!(refusal.contains("Terminal could not open"));
        }

        #[test]
        fn checkpoint_state_distinguishes_native_workspace_truth_from_stable_navigation() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-native-checkpoint-binding-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let source = scratch.join("source");
            let storage = scratch.join("workspace.mesh");
            fs::create_dir(&source).expect("source");
            fs::write(source.join("note.txt"), "native\n").expect("source content");

            let presented_daemon = daemon();
            let preview = presented_daemon
                .preview_folder_import(&source.display().to_string())
                .expect("preview import");
            let summary = preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("preview summary");
            let imported = presented_daemon
                .confirm_folder_import(
                    &source.display().to_string(),
                    &storage.display().to_string(),
                    summary,
                )
                .expect("confirm import");
            let presented = imported
                .get("destination")
                .and_then(Json::as_text)
                .expect("presented folder");
            let state = Json::parse(
                &managed_checkpoint_state_for(&presented_daemon).expect("native checkpoint state"),
            )
            .expect("checkpoint JSON");
            assert_eq!(
                state.get("native_folder").and_then(Json::as_bool),
                Some(true)
            );
            assert_eq!(
                state.get("native_folder_path").and_then(Json::as_text),
                Some(
                    fs::canonicalize(presented)
                        .expect("canonical presented folder")
                        .to_string_lossy()
                        .as_ref()
                )
            );

            let legacy = daemon();
            let legacy_root = scratch.join("legacy");
            legacy
                .open_workspace(&legacy_root.display().to_string())
                .expect("open legacy workspace");
            let legacy_state = Json::parse(
                &managed_checkpoint_state_for(&legacy).expect("legacy checkpoint state"),
            )
            .expect("legacy checkpoint JSON");
            assert_eq!(
                legacy_state.get("native_folder").and_then(Json::as_bool),
                Some(false)
            );
            assert!(matches!(
                legacy_state.get("native_folder_path"),
                Some(Json::Null)
            ));
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn a_legacy_workspace_can_start_a_fresh_isolated_agent_folder() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-legacy-fresh-agent-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let source = scratch.join("ordinary-source");
            let legacy = scratch.join("legacy-workspace");
            let application = scratch.join("app");
            fs::create_dir(&source).expect("ordinary source");
            fs::write(source.join("notes.txt"), "saved legacy point\n")
                .expect("ordinary source content");
            fs::create_dir(&application).expect("application data");
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private application data");

            mesh_daemon::PreparedFolderImport::prepare(&source, &legacy)
                .expect("prepare legacy in-folder workspace")
                .confirm_into_workspace()
                .expect("initialize legacy history");

            let daemon = daemon();
            daemon
                .open_workspace(&legacy.display().to_string())
                .expect("open legacy workspace");
            let legacy_state = daemon.workspace_state().expect("legacy state");
            let selected = legacy_state
                .workspace_versions
                .last()
                .expect("saved workspace point")
                .operation()
                .to_string();
            assert_eq!(
                fs::read_to_string(legacy.join("notes.txt")).unwrap(),
                "saved legacy point\n"
            );
            let verified_legacy = daemon
                .verified_managed_workspace_path(
                    &legacy_state.root,
                    &legacy_state.digest,
                    &legacy_state.installation,
                )
                .expect("verify legacy source");
            assert!(!verified_legacy.is_presented());

            let recent = RecentWorkspace::new(application.clone());
            let active = ActiveWorkspaceLink::new(&application);
            let legacy_navigation = remember_current_workspace_navigation(
                &daemon,
                &recent,
                &active,
                &legacy_state,
                None,
            )
            .expect("remember legacy workspace without inventing a native folder");
            assert!(legacy_navigation.active_folder.is_none());
            assert!(legacy_navigation.export_root.is_none());

            let versions = VersionWorkspaceDirectory::new(&application);
            let answer = open_workspace_version_copy(
                &daemon,
                &versions,
                &selected,
                None,
                None,
                WorkspaceVersionSource {
                    root: &legacy_state.root,
                    digest: &legacy_state.digest,
                    installation: &legacy_state.installation,
                    recent: &recent,
                },
                false,
            )
            .expect("start a fresh agent from the legacy saved point");
            let navigation = commit_opened_workspace_version_navigation(
                &daemon, &recent, &active, &answer, None, None,
            )
            .expect("commit fresh-agent navigation");
            let opened = Json::parse(&answer).expect("fresh-agent response");
            assert_eq!(
                opened.get("action").and_then(Json::as_text),
                Some("workspace-version-opened-as-copy")
            );
            assert_eq!(opened.get("source_ordinal").and_then(Json::as_u64), Some(1));
            assert_eq!(
                recent
                    .load_entries()
                    .expect("point-aware navigation")
                    .first()
                    .and_then(RecentWorkspaceEntry::source_point_ordinal),
                Some(1)
            );
            assert_ne!(opened.get("reused").and_then(Json::as_bool), Some(true));
            let private_store = PathBuf::from(
                opened
                    .get("private_store")
                    .and_then(Json::as_text)
                    .expect("app-owned private store"),
            );
            let presented = PathBuf::from(
                opened
                    .get("destination")
                    .and_then(Json::as_text)
                    .expect("native agent folder"),
            );
            assert_eq!(
                presented,
                private_store.join(mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME)
            );
            assert_eq!(
                private_store.parent(),
                Some(application.join("workspace-versions").as_path())
            );
            assert_eq!(
                fs::read_to_string(presented.join("notes.txt")).unwrap(),
                "saved legacy point\n"
            );
            let stable = PathBuf::from(
                navigation
                    .active_folder
                    .expect("fresh agent activates the stable human path"),
            );
            assert_eq!(
                fs::read_link(&stable).unwrap(),
                fs::canonicalize(&presented).unwrap()
            );
            assert!(navigation.export_root.is_none());

            fs::write(presented.join("agent-work.txt"), "agent-owned edit\n")
                .expect("native agent edit");
            assert!(!legacy.join("agent-work.txt").exists());
            assert_eq!(
                fs::read_to_string(legacy.join("notes.txt")).unwrap(),
                "saved legacy point\n",
                "starting an agent rewrote the legacy source"
            );

            let restarted = self::daemon();
            let reopened = reopen_remembered_workspace(&restarted, &recent, &active);
            assert!(reopened.auto_opened);
            assert_eq!(reopened.active_folder.as_deref(), stable.to_str());
            assert_eq!(
                fs::read_link(&stable).unwrap(),
                fs::canonicalize(&presented).unwrap()
            );
            assert_eq!(
                fs::read_to_string(presented.join("agent-work.txt")).unwrap(),
                "agent-owned edit\n"
            );
            assert!(reopened.export_root.is_none());

            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn codex_context_stays_private_and_never_becomes_agent_work() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-private-codex-context-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let storage = scratch.join("workspace.mesh");
            let presented = mesh_daemon::workspace::OpenWorkspace::open_presented(&storage)
                .expect("create presented workspace")
                .root()
                .as_path()
                .to_path_buf();
            let daemon = daemon();
            let state = daemon
                .open_workspace(&presented.display().to_string())
                .expect("open presented workspace");
            let verified = daemon
                .verified_managed_workspace_path(&state.root, &state.digest, &state.installation)
                .expect("verified workspace");

            let installed = ensure_codex_project_config(
                verified.path(),
                &std::env::current_exe().expect("test executable"),
                &scratch.join("runtime/daemon.sock"),
                &state.installation,
            )
            .expect("prepare launch-scoped Codex context");
            assert_eq!(installed, CodexProjectConfig::Current);
            let refreshed = daemon.workspace_state().expect("refresh native discovery");
            assert!(refreshed.native_untracked_files.is_empty());
            assert!(daemon
                .native_untracked_directories()
                .expect("native directory discovery")
                .is_empty());
            assert!(matches!(
                fs::symlink_metadata(presented.join(".codex")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            ));
            assert!(matches!(
                fs::symlink_metadata(storage.join("integrations/codex")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            ));
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn native_calls_refuse_mismatched_welcomes_before_sending_the_call() {
            for (name, id, session, surface_version) in [
                ("id", 99, DESKTOP_SESSION, SURFACE_VERSION),
                ("session", 1, "another-desktop", SURFACE_VERSION),
                ("surface", 1, DESKTOP_SESSION, SURFACE_VERSION - 1),
            ] {
                let scratch = std::env::temp_dir().join(format!(
                    "mesh-desktop-wrong-welcome-{name}-{}",
                    std::process::id()
                ));
                let _ = fs::remove_dir_all(&scratch);
                fs::create_dir_all(&scratch).expect("scratch");
                let endpoint = scratch.join("daemon.sock");
                let listener = UnixListener::bind(&endpoint).expect("listener");
                let server = thread::spawn(move || {
                    let (mut stream, _) = listener.accept().expect("accept");
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .expect("read timeout");
                    let mut reader = BufReader::new(stream.try_clone().expect("reader"));
                    let mut hello = String::new();
                    reader.read_line(&mut hello).expect("hello");
                    assert!(matches!(
                        ClientMessage::decode(hello.trim_end()),
                        Ok(ClientMessage::Hello { id: 1, .. })
                    ));
                    let wrong = DaemonMessage::Welcome {
                        id,
                        version: SURFACE_VERSION,
                        session: session.to_owned(),
                        resumed: false,
                        surface_version,
                    };
                    writeln!(stream, "{}", wrong.encode()).expect("welcome");
                    stream.flush().expect("flush welcome");

                    let mut call = String::new();
                    let call_was_sent = reader.read_line(&mut call).unwrap_or(0) != 0;
                    if call_was_sent {
                        writeln!(
                            stream,
                            "{}",
                            DaemonMessage::Result {
                                id: 2,
                                value: Json::object([("serving", Json::Bool(true))]),
                            }
                            .encode()
                        )
                        .expect("result");
                        stream.flush().expect("flush result");
                    }
                    call_was_sent
                });

                let result = call_daemon(&endpoint, "daemon.status".to_owned(), "{}".to_owned());
                assert!(result.is_err(), "the {name} mismatch authorized a call");
                assert!(
                    !server.join().expect("server"),
                    "the {name} mismatch reached the peer"
                );
                let _ = fs::remove_dir_all(scratch);
            }
        }

        #[test]
        fn native_call_reassembles_a_large_workspace_reply() {
            let scratch = std::env::temp_dir()
                .join(format!("mesh-desktop-large-reply-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let endpoint = scratch.join("daemon.sock");
            let listener = UnixListener::bind(&endpoint).expect("listener");
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut reader = BufReader::new(stream.try_clone().expect("reader"));
                let mut hello = String::new();
                reader.read_line(&mut hello).expect("hello");
                writeln!(
                    stream,
                    "{}",
                    DaemonMessage::Welcome {
                        id: 1,
                        version: SURFACE_VERSION,
                        session: DESKTOP_SESSION.to_owned(),
                        resumed: false,
                        surface_version: SURFACE_VERSION,
                    }
                    .encode()
                )
                .expect("welcome");
                stream.flush().expect("flush welcome");
                let mut call = String::new();
                reader.read_line(&mut call).expect("call");
                let result = DaemonMessage::Result {
                    id: 2,
                    value: Json::object([(
                        "entries",
                        Json::Array(
                            (0..4_000)
                                .map(|index| Json::text(format!("src/generated-{index}.rs")))
                                .collect(),
                        ),
                    )]),
                };
                for frame in mesh_daemon::ipc::daemon_frames(&result, Some(SURFACE_VERSION)) {
                    writeln!(stream, "{}", frame.encode()).expect("frame");
                }
                stream.flush().expect("flush frames");
            });

            let encoded = call_daemon(&endpoint, "workspace.state".to_owned(), "{}".to_owned())
                .expect("large reply");
            server.join().expect("server");
            let answer = Json::parse(&encoded).expect("answer");
            assert_eq!(
                answer
                    .get("entries")
                    .and_then(Json::as_array)
                    .map(<[_]>::len),
                Some(4_000)
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn filesystem_bound_calls_own_longer_deadlines_without_unbounding_other_calls() {
            assert_eq!(
                daemon_reply_timeout("folder.import.confirm"),
                DAEMON_IMPORT_REPLY_TIMEOUT,
            );
            assert!(DAEMON_IMPORT_REPLY_TIMEOUT > Duration::from_secs(15 * 60));
            for method in ["workspace.state", "workspace.open"] {
                assert_eq!(
                    daemon_reply_timeout(method),
                    DAEMON_WORKSPACE_READ_REPLY_TIMEOUT,
                    "{method}"
                );
            }
            assert!(DAEMON_WORKSPACE_READ_REPLY_TIMEOUT >= Duration::from_secs(5 * 60));
            for method in ["folder.import.preview", "unknown.method"] {
                assert_eq!(
                    daemon_reply_timeout(method),
                    DAEMON_REPLY_TIMEOUT,
                    "{method}"
                );
            }
        }

        #[test]
        fn native_calls_bound_silent_greetings_and_results() {
            for stall_after_welcome in [false, true] {
                let name = if stall_after_welcome {
                    "silent-result"
                } else {
                    "silent-greeting"
                };
                let scratch = std::env::temp_dir()
                    .join(format!("mesh-desktop-{name}-{}", std::process::id()));
                let _ = fs::remove_dir_all(&scratch);
                fs::create_dir_all(&scratch).expect("scratch");
                let endpoint = scratch.join("daemon.sock");
                let listener = UnixListener::bind(&endpoint).expect("listener");
                let server = thread::spawn(move || {
                    let (mut stream, _) = listener.accept().expect("accept");
                    let mut reader = BufReader::new(stream.try_clone().expect("reader"));
                    let mut hello = String::new();
                    reader.read_line(&mut hello).expect("hello");
                    if stall_after_welcome {
                        writeln!(
                            stream,
                            "{}",
                            DaemonMessage::Welcome {
                                id: 1,
                                version: SURFACE_VERSION,
                                session: DESKTOP_SESSION.to_owned(),
                                resumed: false,
                                surface_version: SURFACE_VERSION,
                            }
                            .encode()
                        )
                        .expect("welcome");
                        stream.flush().expect("welcome flush");
                        let mut call = String::new();
                        reader.read_line(&mut call).expect("call");
                    }
                    thread::sleep(Duration::from_millis(150));
                });

                let started = std::time::Instant::now();
                let error = call_daemon_with_timeout(
                    &endpoint,
                    "workspace.state".to_owned(),
                    "{}".to_owned(),
                    Duration::from_millis(25),
                )
                .expect_err("silent peer must not pin the desktop command forever");
                assert!(
                    error.contains("did not answer before the desktop reply deadline"),
                    "{name}: {error}"
                );
                assert!(
                    started.elapsed() < Duration::from_secs(1),
                    "{name} was not bounded"
                );
                server.join().expect("server");
                let _ = fs::remove_dir_all(scratch);
            }
        }

        #[test]
        fn native_calls_preserve_the_daemon_refusal_code() {
            let scratch = std::env::temp_dir()
                .join(format!("mesh-desktop-coded-refusal-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let endpoint = scratch.join("daemon.sock");
            let listener = UnixListener::bind(&endpoint).expect("listener");
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut reader = BufReader::new(stream.try_clone().expect("reader"));
                let mut hello = String::new();
                reader.read_line(&mut hello).expect("hello");
                writeln!(
                    stream,
                    "{}",
                    DaemonMessage::Welcome {
                        id: 1,
                        version: SURFACE_VERSION,
                        session: DESKTOP_SESSION.to_owned(),
                        resumed: false,
                        surface_version: SURFACE_VERSION,
                    }
                    .encode()
                )
                .expect("welcome");
                stream.flush().expect("flush welcome");

                let mut call = String::new();
                reader.read_line(&mut call).expect("call");
                writeln!(
                    stream,
                    "{}",
                    DaemonMessage::Failed {
                        id: 2,
                        code: "no-workspace-open".to_owned(),
                        message: "The human wording may change".to_owned(),
                    }
                    .encode()
                )
                .expect("failure");
                stream.flush().expect("flush failure");
            });

            let encoded = call_daemon(&endpoint, "workspace.state".to_owned(), "{}".to_owned())
                .expect_err("workspace refusal");
            server.join().expect("server");
            let refusal = Json::parse(&encoded).expect("structured refusal");
            assert_eq!(
                refusal.get("kind").and_then(Json::as_text),
                Some(DAEMON_REFUSAL_KIND)
            );
            assert_eq!(
                refusal.get("code").and_then(Json::as_text),
                Some("no-workspace-open")
            );
            assert_eq!(
                refusal.get("message").and_then(Json::as_text),
                Some("The human wording may change")
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn a_second_native_host_reopens_the_exact_remembered_workspace() {
            let scratch = std::env::temp_dir()
                .join(format!("mesh-desktop-auto-reopen-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let private_workspace = scratch.join("workspace.mesh");
            let presented =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&private_workspace)
                    .expect("create presented workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            let first = daemon();
            let opened = first
                .open_workspace(&presented.display().to_string())
                .expect("first open");
            let recent = RecentWorkspace::new(scratch.join("app"));
            let earlier = scratch.join("earlier-workspace");
            let original = scratch.join("ordinary-project");
            fs::create_dir(&earlier).expect("earlier workspace");
            fs::create_dir(&original).expect("ordinary project");
            let original = fs::canonicalize(original).expect("canonical ordinary project");
            let earlier = recent.remember(&earlier).expect("remember earlier");
            let canonical = recent
                .remember_with_export_root(std::path::Path::new(&opened.root), Some(&original))
                .expect("remember");
            drop(first);

            let restarted = daemon();
            let active = ActiveWorkspaceLink::new(&scratch.join("app"));
            let status = reopen_remembered_workspace(&restarted, &recent, &active);
            assert!(status.auto_opened);
            assert_eq!(status.warning, None);
            let active_path = status.active_folder.as_ref().expect("stable native folder");
            assert_eq!(
                fs::read_link(active_path).expect("read stable native folder"),
                canonical
            );
            assert_eq!(status.remembered, Some(canonical.display().to_string()));
            assert_eq!(status.export_root, Some(original.display().to_string()));
            assert_eq!(
                status.workspaces,
                vec![
                    canonical.display().to_string(),
                    earlier.display().to_string()
                ]
            );
            let encoded = Json::parse(&status.to_json()).expect("recent status JSON");
            let entries = encoded
                .get("workspace_entries")
                .and_then(Json::as_array)
                .expect("workspace navigation entries");
            assert_eq!(entries.len(), 2);
            assert_eq!(
                entries[0].get("path").and_then(Json::as_text),
                Some(canonical.to_str().expect("UTF-8 canonical workspace"))
            );
            assert_eq!(
                entries[0].get("export_root").and_then(Json::as_text),
                Some(original.to_str().expect("UTF-8 ordinary project"))
            );
            assert_eq!(
                entries[1].get("path").and_then(Json::as_text),
                Some(earlier.to_str().expect("UTF-8 earlier workspace"))
            );
            assert_eq!(entries[1].get("export_root"), Some(&Json::Null));
            assert_eq!(
                restarted.workspace_state().expect("reopened").root,
                canonical.display().to_string()
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn failed_startup_reopen_deactivates_the_prior_stable_folder() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-stale-startup-navigation-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let private_workspace = scratch.join("workspace.mesh");
            let presented =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&private_workspace)
                    .expect("create presented workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            let recent = RecentWorkspace::new(scratch.join("app"));
            let active = ActiveWorkspaceLink::new(&scratch.join("app"));
            recent.remember(&presented).expect("remember workspace");
            let stable = active.activate(&presented).expect("stable native folder");
            assert_eq!(fs::read_link(&stable).expect("read stable link"), presented);

            fs::remove_dir_all(&private_workspace).expect("remove remembered workspace");
            let restarted = daemon();
            let status = reopen_remembered_workspace(&restarted, &recent, &active);

            assert!(!status.auto_opened);
            assert!(status.active_folder.is_none());
            assert!(status
                .warning
                .as_deref()
                .is_some_and(|warning| { warning.contains("could not be reopened safely") }));
            assert_eq!(
                fs::symlink_metadata(&stable)
                    .expect_err("failed reopen must remove stale stable navigation")
                    .kind(),
                std::io::ErrorKind::NotFound
            );
            assert_eq!(
                fs::symlink_metadata(&private_workspace)
                    .expect_err("automatic reopen must not recreate a missing workspace")
                    .kind(),
                std::io::ErrorKind::NotFound
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn startup_reopen_never_recreates_a_missing_workspace_journal() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-missing-startup-journal-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let opened =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&scratch.join("workspace"))
                    .expect("create presented workspace");
            let presented = opened.root().as_path().to_path_buf();
            let journal = opened.record_file().to_path_buf();
            drop(opened);
            let recent = RecentWorkspace::new(scratch.join("app"));
            let active = ActiveWorkspaceLink::new(&scratch.join("app"));
            recent.remember(&presented).expect("remember workspace");
            let stable = active.activate(&presented).expect("stable native folder");
            fs::remove_file(&journal).expect("remove remembered journal");

            let status = reopen_remembered_workspace(&daemon(), &recent, &active);

            assert!(!status.auto_opened);
            assert!(status.active_folder.is_none());
            assert!(status
                .warning
                .as_deref()
                .is_some_and(|warning| warning.contains("could not be reopened safely")));
            assert!(
                !journal.exists(),
                "automatic reopen must not create a journal"
            );
            assert_eq!(
                fs::symlink_metadata(stable)
                    .expect_err("refused journal must deactivate stable navigation")
                    .kind(),
                std::io::ErrorKind::NotFound
            );
            assert!(presented.is_dir(), "refusal must preserve native files");
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn empty_recent_history_deactivates_the_prior_stable_folder() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-empty-startup-navigation-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let target = scratch.join("old-workspace");
            fs::create_dir(&target).expect("old workspace");
            let target = fs::canonicalize(target).expect("canonical old workspace");
            let app = scratch.join("app");
            fs::create_dir(&app).expect("app data");
            fs::set_permissions(
                &app,
                <fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o700),
            )
            .expect("secure app data");
            let recent = RecentWorkspace::new(app.clone());
            let active = ActiveWorkspaceLink::new(&app);
            let stable = active.activate(&target).expect("stable native folder");

            let status = reopen_remembered_workspace(&daemon(), &recent, &active);

            assert!(!status.auto_opened);
            assert!(status.remembered.is_none());
            assert!(status.active_folder.is_none());
            assert_eq!(status.warning, None);
            assert_eq!(
                fs::symlink_metadata(&stable)
                    .expect_err("empty history must remove stale navigation")
                    .kind(),
                std::io::ErrorKind::NotFound
            );
            assert!(target.is_dir(), "deactivation must preserve the target");
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn refresh_retargets_stable_navigation_after_an_external_workspace_switch() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-external-workspace-switch-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let first =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&scratch.join("first.mesh"))
                    .expect("first presented workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            let second =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&scratch.join("second.mesh"))
                    .expect("second presented workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            fs::write(first.join("version.txt"), "one\n").expect("first version");
            fs::write(second.join("version.txt"), "two\n").expect("second version");
            let daemon = daemon();
            let active = ActiveWorkspaceLink::new(&scratch.join("app"));
            fs::create_dir(scratch.join("app")).expect("application data");

            let first_state = daemon
                .open_workspace(&first.display().to_string())
                .expect("open first workspace");
            let (stable, first_real) = reconcile_verified_native_folder(
                &daemon,
                &active,
                &first_state.root,
                &first_state.digest,
                &first_state.installation,
            )
            .expect("activate first workspace");
            let stable = stable.expect("stable path");
            assert_eq!(first_real, first);
            assert_eq!(fs::read_link(&stable).unwrap(), first);

            let second_state = daemon
                .open_workspace(&second.display().to_string())
                .expect("external client opens second workspace");
            reconcile_verified_native_folder(
                &daemon,
                &active,
                &first_state.root,
                &first_state.digest,
                &first_state.installation,
            )
            .expect_err("stale desktop binding must not retarget navigation");
            assert_eq!(fs::read_link(&stable).unwrap(), first);

            let (same_stable, second_real) = reconcile_verified_native_folder(
                &daemon,
                &active,
                &second_state.root,
                &second_state.digest,
                &second_state.installation,
            )
            .expect("refresh retargets to second workspace");
            assert_eq!(same_stable.as_deref(), Some(stable.as_path()));
            assert_eq!(second_real, second);
            assert_eq!(fs::read_link(&stable).unwrap(), second);
            assert_eq!(
                fs::read_to_string(stable.join("version.txt")).unwrap(),
                "two\n"
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn successful_startup_migrates_the_legacy_navigation_record() {
            use std::os::unix::fs::PermissionsExt as _;

            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-auto-migrate-navigation-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let private_workspace = scratch.join("workspace.mesh");
            let presented =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&private_workspace)
                    .expect("create presented workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            let application = scratch.join("app");
            fs::create_dir(&application).expect("application data");
            fs::set_permissions(&application, fs::Permissions::from_mode(0o700))
                .expect("private application data");
            let record = application.join("recent-workspace.json");
            fs::write(
                &record,
                Json::object([
                    ("schema", Json::text("mesh-desktop-recent-workspace/v1")),
                    ("path", Json::text(presented.to_string_lossy())),
                ])
                .encode(),
            )
            .expect("legacy record");
            fs::set_permissions(&record, fs::Permissions::from_mode(0o600))
                .expect("private record");

            let daemon = daemon();
            let recent = RecentWorkspace::new(application.clone());
            let active = ActiveWorkspaceLink::new(&application);
            let status = reopen_remembered_workspace(&daemon, &recent, &active);
            assert!(status.auto_opened);
            assert_eq!(status.warning, None);
            assert_eq!(status.workspaces, vec![presented.display().to_string()]);
            assert_eq!(
                fs::read_to_string(&record).unwrap(),
                Json::object([
                    ("schema", Json::text("mesh-desktop-recent-workspaces/v9"),),
                    (
                        "workspaces",
                        Json::Array(vec![Json::object([
                            ("path", Json::text(presented.to_string_lossy())),
                            ("export_root", Json::Null),
                            ("project_root", Json::Null),
                            ("agent_handoff_installation", Json::Null),
                            ("agent_handoff_directory", Json::Null),
                            ("agent_handoff_generation", Json::Null),
                            ("source_point_ordinal", Json::Null),
                            ("original_update_version", Json::Null),
                        ])]),
                    )
                ])
                .encode()
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn corrupt_recent_history_cannot_leave_the_stable_path_on_an_older_workspace() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-history-failure-navigation-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let first =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&scratch.join("first.mesh"))
                    .expect("first presented workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            let second =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&scratch.join("second.mesh"))
                    .expect("second presented workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            let daemon = daemon();
            let application = scratch.join("app");
            fs::create_dir(&application).expect("application data");
            let recent = RecentWorkspace::new(application.clone());
            let active = ActiveWorkspaceLink::new(&application);

            daemon
                .open_workspace(&first.display().to_string())
                .expect("open first workspace");
            let first_state = daemon.workspace_state().expect("first workspace state");
            let initial = remember_current_workspace_navigation(
                &daemon,
                &recent,
                &active,
                &first_state,
                None,
            )
            .expect("remember first workspace");
            let stable = PathBuf::from(initial.active_folder.expect("first stable path"));
            assert_eq!(fs::read_link(&stable).unwrap(), first);

            fs::write(
                application.join("recent-workspace.json"),
                "not canonical JSON",
            )
            .expect("corrupt recent history");
            daemon
                .open_workspace(&second.display().to_string())
                .expect("open second workspace");
            let second_state = daemon.workspace_state().expect("second workspace state");
            let switched = remember_current_workspace_navigation(
                &daemon,
                &recent,
                &active,
                &second_state,
                None,
            )
            .expect("stable navigation remains available");
            assert_eq!(switched.workspaces, vec![second.display().to_string()]);
            assert_eq!(switched.remembered.as_deref(), second.to_str());
            assert!(switched.warning.as_deref().is_some_and(|warning| {
                warning.contains("replaced malformed app-owned recent-workspace content")
            }));
            assert_eq!(switched.active_folder.as_deref(), stable.to_str());
            assert_eq!(fs::read_link(&stable).unwrap(), second);
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn stable_navigation_refuses_a_workspace_replaced_after_daemon_verification() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-replaced-native-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let private_workspace = scratch.join("workspace.mesh");
            let presented =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&private_workspace)
                    .expect("create presented workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            let daemon = daemon();
            let state = daemon
                .open_workspace(&presented.display().to_string())
                .expect("open workspace");
            let verified = daemon
                .verified_managed_workspace_path(&state.root, &state.digest, &state.installation)
                .expect("verify workspace");

            let prior = fs::canonicalize(&scratch)
                .expect("canonical scratch")
                .join("prior");
            fs::create_dir(&prior).expect("prior workspace");
            fs::create_dir(scratch.join("app")).expect("application data");
            fs::write(prior.join("version.txt"), "prior\n").expect("prior content");
            let active = ActiveWorkspaceLink::new(&scratch.join("app"));
            let stable = active
                .activate(&prior)
                .expect("activate the previously selected workspace");
            assert_eq!(fs::read_link(&stable).unwrap(), prior);

            fs::rename(&presented, private_workspace.join("displaced-mounts"))
                .expect("displace workspace");
            fs::create_dir(&presented).expect("replacement workspace");
            fs::write(presented.join("foreign.txt"), "replacement\n").expect("replacement content");
            let error = activate_verified_native_folder(&active, &verified)
                .expect_err("replacement must not become the stable native folder");
            assert!(error.contains("changed before activation"), "{error}");
            assert_eq!(
                fs::read_link(&stable).expect("unrelated stable selection remains"),
                prior,
                "a stale activation failure must not remove another verified workspace",
            );
            assert_eq!(
                fs::read_to_string(presented.join("foreign.txt")).unwrap(),
                "replacement\n"
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn git_import_and_agent_version_use_independent_native_repositories() {
            let scratch = std::env::temp_dir()
                .join(format!("mesh-desktop-git-agent-e2e-{}", std::process::id()));
            let _ = fs::remove_dir_all(&scratch);
            let source = scratch.join("source");
            let private_workspace = scratch.join("current.mesh");
            let application = scratch.join("app");
            fs::create_dir_all(&source).expect("source");
            fs::create_dir(&application).expect("application data");
            fs::write(source.join("notes.txt"), "original\n").expect("source file");
            fs::write(source.join(".gitignore"), "node_modules/\n").expect("ignore rules");
            fs::create_dir(source.join("node_modules")).expect("ignored dependency directory");
            fs::write(
                source.join("node_modules/cache.bin"),
                b"ignored dependency bytes",
            )
            .expect("ignored dependency");
            let run_git = |root: &std::path::Path, arguments: &[&str]| -> Vec<u8> {
                let output = Command::new("git")
                    .arg("-C")
                    .arg(root)
                    .args(arguments)
                    .env("GIT_CONFIG_NOSYSTEM", "1")
                    .env("GIT_CONFIG_GLOBAL", "/dev/null")
                    .output()
                    .expect("run Git fixture command");
                assert!(
                    output.status.success(),
                    "git {arguments:?}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                output.stdout
            };
            run_git(&source, &["init", "-b", "main"]);
            run_git(&source, &["config", "user.name", "Mesh test"]);
            run_git(&source, &["config", "user.email", "mesh@example.invalid"]);
            run_git(&source, &["add", "notes.txt", ".gitignore"]);
            run_git(&source, &["commit", "-m", "original"]);
            fs::write(source.join("notes.txt"), "staged intermediate\n")
                .expect("staged source state");
            run_git(&source, &["add", "notes.txt"]);
            fs::write(source.join("notes.txt"), "original\n")
                .expect("unstaged source state after index update");

            let daemon = Arc::new(daemon());
            let preview = daemon
                .preview_folder_import(&source.display().to_string())
                .expect("preview import");
            let summary = preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("summary");
            let imported = daemon
                .confirm_folder_import(
                    &source.display().to_string(),
                    &private_workspace.display().to_string(),
                    summary,
                )
                .expect("import");
            let current = PathBuf::from(
                imported
                    .get("destination")
                    .and_then(Json::as_text)
                    .expect("presented folder"),
            );
            let imported = install_prepared_git_context(
                &source,
                &imported.encode(),
                prepare_git_context(&source),
                true,
                &git_setup_destination_identity(&current).expect("pin imported destination"),
            )
            .expect("install imported Git context");
            assert_eq!(
                Json::parse(&imported)
                    .unwrap()
                    .get("git_context")
                    .and_then(Json::as_text),
                Some("installed")
            );
            assert!(current.join(".git").is_dir());
            assert!(
                !current.join("node_modules").exists(),
                "Mesh import must not recreate Git-ignored dependency state"
            );
            let source_head = fs::read(source.join(".git/HEAD")).expect("source head");

            let state = daemon.workspace_state().expect("imported state");
            let operation = state.workspace_versions[0].operation().to_string();
            let recent = RecentWorkspace::new(application.clone());
            let active = ActiveWorkspaceLink::new(&application);
            let first_navigation = remember_current_workspace_navigation_with_project_root(
                &daemon,
                &recent,
                &active,
                &state,
                Some(&source),
                Some(&source),
                None,
            )
            .expect("initial navigation");
            let stable = PathBuf::from(first_navigation.active_folder.expect("stable path"));
            let canonical_current = fs::canonicalize(&current).expect("canonical imported root");
            let remembered = recent.load_entries().expect("remembered import");
            assert_eq!(remembered[0].path(), canonical_current);
            assert_eq!(
                remembered[0].project_root(),
                Some(fs::canonicalize(&source).unwrap().as_path())
            );
            fs::remove_dir_all(canonical_current.join(".git"))
                .expect("simulate a workspace imported by the previous alpha build");
            let current_git_identity =
                git_setup_destination_identity(&canonical_current).expect("pin source");
            let generation = daemon
                .acquire_workspace_agent_custody(
                    &state.root,
                    &state.digest,
                    &state.installation,
                    false,
                    None,
                )
                .expect("assign the workspace without updating the stale Recent mirror");
            ensure_git_context_for_unassigned_workspace(
                &daemon,
                &recent,
                &canonical_current,
                &state.root,
                &state.digest,
                &state.installation,
            )
            .expect_err("an assigned source must not receive Git setup during a version switch");
            assert!(
                !canonical_current.join(".git").exists(),
                "stale Recent navigation must not let version switching mutate an agent folder"
            );
            with_agent_setup_authority(
                &daemon,
                &state.root,
                &state.digest,
                &state.installation,
                "ffffffffffffffffffffffffffffffff",
                || {
                    let source = Ok(None);
                    Ok(ensure_git_context_for_source(
                        &canonical_current,
                        &source,
                        &current_git_identity,
                    ))
                },
            )
            .expect_err("only the exact acquired generation may prepare an agent folder");
            assert!(
                !canonical_current.join(".git").exists(),
                "a stale agent generation must not publish Git metadata"
            );
            let git_source = git_context_source_for_path(&recent, &canonical_current);
            assert_eq!(
                with_agent_setup_authority(
                    &daemon,
                    &state.root,
                    &state.digest,
                    &state.installation,
                    &generation,
                    || Ok(ensure_git_context_for_source(
                        &canonical_current,
                        &git_source,
                        &current_git_identity,
                    )),
                )
                .expect("exact assigned generation may prepare the folder before launch"),
                ("installed", None),
                "first native use should upgrade a remembered Git import without re-importing it"
            );
            assert!(canonical_current.join(".git").is_dir());
            assert!(
                String::from_utf8(run_git(&canonical_current, &["status", "--porcelain=v1"]))
                    .unwrap()
                    .lines()
                    .any(|line| line == "MM notes.txt"),
                "legacy upgrade must preserve the staged and then edited index state"
            );
            assert!(daemon
                .release_workspace_agent_custody(
                    &state.root,
                    &state.digest,
                    &state.installation,
                    &generation,
                )
                .expect("release exact setup generation"));
            let versions = VersionWorkspaceDirectory::new(&application);
            let opened = open_workspace_version_copy(
                &daemon,
                &versions,
                &operation,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &state.root,
                    digest: &state.digest,
                    installation: &state.installation,
                    recent: &recent,
                },
                false,
            )
            .expect("fresh agent version");
            let navigation = commit_opened_workspace_version_navigation(
                &daemon,
                &recent,
                &active,
                &opened,
                Some(&source),
                Some(&source),
            )
            .expect("switch stable native path");
            assert_eq!(navigation.active_folder.as_deref(), stable.to_str());
            let opened = Json::parse(&opened).expect("opened response");
            assert_eq!(
                opened.get("git_context").and_then(Json::as_text),
                Some("installed")
            );
            let agent = PathBuf::from(
                opened
                    .get("destination")
                    .and_then(Json::as_text)
                    .expect("agent folder"),
            );
            let destination_without_git = match Json::parse(&opened.encode()).unwrap() {
                Json::Object(fields) => Json::Object(
                    fields
                        .into_iter()
                        .filter(|(name, _)| name != "git_context" && name != "git_context_warning")
                        .collect(),
                )
                .encode(),
                _ => panic!("opened version response must be an object"),
            };
            fs::remove_dir_all(agent.join(".git"))
                .expect("simulate a destination before Git publication");
            let agent_state = daemon.workspace_state().expect("opened agent state");
            let agent_generation = daemon
                .acquire_workspace_agent_custody(
                    &agent_state.root,
                    &agent_state.digest,
                    &agent_state.installation,
                    false,
                    None,
                )
                .expect("assign destination before a stale installer resumes");
            install_prepared_git_context_for_open_workspace(
                &daemon,
                &source,
                &destination_without_git,
                prepare_git_context(&source),
                false,
            )
            .expect_err("an assigned destination must refuse delayed Git publication");
            assert!(
                !agent.join(".git").exists(),
                "delayed destination setup must not mutate the active agent folder"
            );
            assert!(daemon
                .release_workspace_agent_custody(
                    &agent_state.root,
                    &agent_state.digest,
                    &agent_state.installation,
                    &agent_generation,
                )
                .expect("release assigned destination"));
            let displaced_agent = agent.with_file_name("displaced-agent.mesh");
            let agent_for_hook = agent.clone();
            let displaced_for_hook = displaced_agent.clone();
            BEFORE_PREPARED_GIT_INSTALL.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    fs::rename(&agent_for_hook, &displaced_for_hook)
                        .expect("displace admitted agent folder");
                    fs::create_dir(&agent_for_hook).expect("create replacement agent folder");
                    fs::write(agent_for_hook.join("replacement.txt"), "replacement\n")
                        .expect("replacement marker");
                }));
            });
            install_prepared_git_context_for_open_workspace(
                &daemon,
                &source,
                &destination_without_git,
                prepare_git_context(&source),
                false,
            )
            .expect_err("replacement after daemon admission must refuse Git publication");
            assert!(
                !agent.join(".git").exists(),
                "replacement folder must never receive Git metadata"
            );
            fs::remove_dir_all(&agent).expect("remove replacement folder");
            fs::rename(&displaced_agent, &agent).expect("restore admitted agent folder");
            let reopened = install_prepared_git_context_for_open_workspace(
                &daemon,
                &source,
                &destination_without_git,
                prepare_git_context(&source),
                false,
            )
            .expect("unassigned destination accepts exact Git publication");
            assert_eq!(
                Json::parse(&reopened)
                    .unwrap()
                    .get("git_context")
                    .and_then(Json::as_text),
                Some("installed")
            );
            assert_eq!(
                fs::read_link(&stable).unwrap(),
                fs::canonicalize(&agent).unwrap()
            );
            assert_ne!(
                fs::canonicalize(current.join(".git")).unwrap(),
                fs::canonicalize(agent.join(".git")).unwrap()
            );
            run_git(&agent, &["checkout", "-b", "agent-version"]);
            assert_eq!(
                String::from_utf8(run_git(&agent, &["branch", "--show-current"]))
                    .unwrap()
                    .trim(),
                "agent-version"
            );
            assert_eq!(
                String::from_utf8(run_git(&current, &["branch", "--show-current"]))
                    .unwrap()
                    .trim(),
                "main"
            );
            let (approved_target, approved_anchor) =
                approved_git_target(&recent, &agent).expect("bind approved Git target");
            assert_eq!(approved_target, fs::canonicalize(&source).unwrap());
            assert_eq!(
                approved_anchor.head().as_str(),
                String::from_utf8(run_git(&source, &["rev-parse", "HEAD"]))
                    .unwrap()
                    .trim()
            );
            assert_eq!(
                fs::read(source.join(".git/HEAD")).expect("source Git unchanged"),
                source_head
            );

            let returned = daemon
                .open_workspace(&current.display().to_string())
                .expect("return to imported version");
            let reused = open_workspace_version_copy(
                &daemon,
                &versions,
                &operation,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &returned.root,
                    digest: &returned.digest,
                    installation: &returned.installation,
                    recent: &recent,
                },
                true,
            )
            .expect("reuse Git-backed version folder");
            let reused = Json::parse(&reused).expect("reused response");
            assert_eq!(
                reused.get("action").and_then(Json::as_text),
                Some("workspace-version-reopened-existing-copy")
            );
            assert_eq!(reused.get("source_ordinal").and_then(Json::as_u64), Some(1));
            assert_eq!(
                reused.get("destination").and_then(Json::as_text),
                agent.to_str()
            );
            assert_eq!(
                reused.get("git_context").and_then(Json::as_text),
                Some("reused")
            );
            assert_eq!(
                String::from_utf8(run_git(&agent, &["branch", "--show-current"]))
                    .unwrap()
                    .trim(),
                "agent-version",
                "reusing a Mesh version must preserve that version's independent Git branch"
            );
            fs::write(agent.join("notes.txt"), "agent edit\n").expect("native agent edit");
            assert_eq!(
                fs::read_to_string(stable.join("notes.txt")).unwrap(),
                "agent edit\n"
            );
            assert_eq!(
                fs::read_to_string(current.join("notes.txt")).unwrap(),
                "original\n"
            );
            assert_eq!(
                fs::read_to_string(source.join("notes.txt")).unwrap(),
                "original\n"
            );
            let _ = fs::remove_dir_all(&scratch);
        }

        #[test]
        fn native_agent_edit_version_switch_and_export_use_one_verified_stable_path() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-native-agent-e2e-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let source = scratch.join("source");
            let private_workspace = scratch.join("current.mesh");
            fs::create_dir_all(&source).expect("source");
            let application = scratch.join("app");
            fs::create_dir(&application).expect("application data");
            fs::write(source.join("notes.txt"), "original\n").expect("source file");
            let daemon = Arc::new(daemon());
            let preview = daemon
                .preview_folder_import(&source.display().to_string())
                .expect("preview import");
            let summary = preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("import summary");
            let imported = daemon
                .confirm_folder_import(
                    &source.display().to_string(),
                    &private_workspace.display().to_string(),
                    summary,
                )
                .expect("confirm import");
            let current = PathBuf::from(
                imported
                    .get("destination")
                    .and_then(Json::as_text)
                    .expect("presented folder"),
            );
            let first = daemon
                .workspace_state()
                .expect("initial state")
                .workspace_versions[0]
                .operation()
                .to_string();
            let state = daemon.workspace_state().expect("displayed state");
            let recent = RecentWorkspace::new(application.clone());
            let active = ActiveWorkspaceLink::new(&application);
            let initial_navigation = remember_current_workspace_navigation_with_project_root(
                &daemon,
                &recent,
                &active,
                &state,
                Some(&source),
                Some(&source),
                None,
            )
            .expect("remember current workspace navigation");
            let stable = PathBuf::from(
                initial_navigation
                    .active_folder
                    .expect("activate stable native folder"),
            );
            assert_eq!(
                fs::read_link(&stable).unwrap(),
                fs::canonicalize(&current).expect("canonical current workspace")
            );

            fs::write(stable.join("notes.txt"), "edited through stable path\n")
                .expect("agent tracked edit");
            fs::write(stable.join("agent-new.txt"), "created by local agent\n")
                .expect("agent new file");
            let author = SoftwareActorCustody::generate().expect("local author");
            let public = author.public_key().public_key();
            let changed = daemon
                .inspect_managed_file("notes.txt")
                .expect("inspect tracked agent edit");
            assert!(changed.modified_from_current_version());
            daemon
                .save_managed_file_privately(
                    "notes.txt",
                    changed.content_digest(),
                    changed.executable(),
                    public,
                    |payload| author.sign(payload),
                )
                .expect("save tracked agent edit");
            let discovered = daemon.workspace_state().expect("discover new agent file");
            assert_eq!(discovered.native_untracked_files, vec!["agent-new.txt"]);
            let new_file = daemon
                .inspect_native_untracked_file("agent-new.txt")
                .expect("inspect new agent file");
            daemon
                .adopt_native_file_privately(
                    new_file.path(),
                    new_file.content_digest(),
                    new_file.executable(),
                    public,
                    |payload| author.sign(payload),
                )
                .expect("save new agent file");

            let current_state = daemon.workspace_state().expect("current agent result");
            let versions = VersionWorkspaceDirectory::new(&application);
            let opened_answer = open_workspace_version_copy(
                &daemon,
                &versions,
                &first,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &current_state.root,
                    digest: &current_state.digest,
                    installation: &current_state.installation,
                    recent: &recent,
                },
                true,
            )
            .expect("open earlier version");
            let switched_navigation = commit_opened_workspace_version_navigation(
                &daemon,
                &recent,
                &active,
                &opened_answer,
                Some(&source),
                Some(&source),
            )
            .expect("commit version navigation before returning success");
            assert_eq!(
                switched_navigation.active_folder.as_deref(),
                stable.to_str()
            );
            assert_eq!(
                switched_navigation.export_root.as_deref(),
                fs::canonicalize(&source).unwrap().to_str()
            );
            let opened = Json::parse(&opened_answer).expect("version response");
            let earlier_presented = PathBuf::from(
                opened
                    .get("destination")
                    .and_then(Json::as_text)
                    .expect("app-managed presented folder"),
            );
            let earlier_workspace = PathBuf::from(
                opened
                    .get("private_store")
                    .and_then(Json::as_text)
                    .expect("app-managed private store"),
            );
            assert_eq!(
                earlier_workspace.parent(),
                Some(application.join("workspace-versions").as_path())
            );
            let earlier_state = daemon.workspace_state().expect("earlier state");
            assert_eq!(PathBuf::from(&earlier_state.root), earlier_presented);
            assert_eq!(
                earlier_presented,
                earlier_workspace.join(mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME)
            );
            let earlier_canonical = fs::canonicalize(&earlier_presented).unwrap();
            assert_eq!(fs::read_link(&stable).unwrap(), earlier_canonical);

            // There is deliberately no browser-side remember or link call between the native
            // switch above and this fresh process. A stopped webview must not strand the checkout.
            let restarted_daemon = self::daemon();
            let restarted = reopen_remembered_workspace(&restarted_daemon, &recent, &active);
            assert!(restarted.auto_opened);
            assert_eq!(restarted.remembered.as_deref(), earlier_canonical.to_str());
            assert_eq!(restarted.active_folder.as_deref(), stable.to_str());
            assert_eq!(fs::read_link(&stable).unwrap(), earlier_canonical);
            assert_eq!(
                restarted_daemon.workspace_state().unwrap().root,
                earlier_canonical.display().to_string()
            );
            assert_eq!(
                fs::read_to_string(stable.join("notes.txt")).unwrap(),
                "original\n"
            );
            assert!(!stable.join("agent-new.txt").exists());
            assert_eq!(
                fs::read_to_string(current.join("notes.txt")).unwrap(),
                "edited through stable path\n"
            );
            drop(restarted_daemon);

            // Older alpha builds installed one verified navigation-only link to private project
            // configuration. Keep that exact historical shape readable and refreshable without
            // teaching current agent launches to add `.codex` to an ordinary Git worktree.
            let legacy_codex_directory = earlier_workspace.join("integrations/codex");
            fs::create_dir_all(&legacy_codex_directory).expect("legacy Codex directory");
            fs::set_permissions(
                earlier_workspace.join("integrations"),
                fs::Permissions::from_mode(0o700),
            )
            .expect("legacy integrations permissions");
            fs::set_permissions(&legacy_codex_directory, fs::Permissions::from_mode(0o700))
                .expect("legacy Codex permissions");
            std::os::unix::fs::symlink("../integrations/codex", earlier_presented.join(".codex"))
                .expect("legacy Codex project link");
            let executable = scratch.join("mesh-desktop-test-executable");
            fs::write(&executable, "test executable\n").expect("test executable");
            let endpoint = application.join("runtime/daemon.sock");
            let codex = ensure_codex_project_config(
                &earlier_presented,
                &executable,
                &endpoint,
                &earlier_state.installation,
            )
            .expect("install exact private Codex context");
            assert_eq!(codex, CodexProjectConfig::Created);
            assert_eq!(
                fs::read_link(earlier_presented.join(".codex")).unwrap(),
                std::path::Path::new("../integrations/codex")
            );

            // A long-running agent can rewrite the one expected file through the project link.
            // Merely checking that the private directory has one correctly named regular file is
            // insufficient: a later ordinary version switch must refresh Mesh's exact binding
            // before the stable native folder exposes this checkout again.
            let private_codex_config = earlier_workspace.join("integrations/codex/config.toml");
            fs::write(
                &private_codex_config,
                "[mcp_servers.attacker]\ncommand = \"/tmp/not-mesh\"\n",
            )
            .expect("replace the sole private Codex configuration through the agent boundary");
            let runtime = DesktopRuntime {
                endpoint: endpoint.clone(),
                daemon: Arc::clone(&daemon),
                author: Mutex::new(SoftwareActorCustody::generate().expect("runtime author")),
                active_workspace: ActiveWorkspaceLink::new(&application),
                version_workspaces: VersionWorkspaceDirectory::new(&application),
                native_capture: NativeCapturePreference::new(&application),
                recent: RecentWorkspace::new(application.clone()),
                recent_status: Mutex::new(RecentWorkspaceStatus::empty()),
                renderer_proof: RendererProofRuntime::disabled(),
                _server: Mutex::new(None),
            };

            let reopened = daemon
                .open_workspace(&current.display().to_string())
                .expect("switch back to current workspace");
            let verified_current = daemon
                .verified_managed_workspace_path(
                    &reopened.root,
                    &reopened.digest,
                    &reopened.installation,
                )
                .expect("reverify current workspace");
            activate_verified_native_folder(&active, &verified_current)
                .expect("retarget stable path to current workspace");
            assert_eq!(
                fs::read_to_string(stable.join("agent-new.txt")).unwrap(),
                "created by local agent\n"
            );

            // Simulate a native-host stop after creating the app-owned lookup hint but before its
            // complete bytes became durable. The next exact open must find and validate the
            // checkout, then publish a separate create-only recovery hint without replacing the
            // interrupted marker or leaking a duplicate point-2 directory.
            let source_marker = earlier_workspace.join(".mesh-source-version");
            fs::write(&source_marker, b"mesh.workspace-version-source/1\n")
                .expect("leave interrupted source marker");
            assert_eq!(
                fs::read(&source_marker).unwrap(),
                b"mesh.workspace-version-source/1\n",
            );

            let current_for_reuse = daemon.workspace_state().expect("current reuse source");
            let reused_answer = open_workspace_version_copy(
                &daemon,
                &versions,
                &first,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &current_for_reuse.root,
                    digest: &current_for_reuse.digest,
                    installation: &current_for_reuse.installation,
                    recent: &recent,
                },
                true,
            )
            .expect("reuse clean earlier version");
            let reused_answer = finish_opened_workspace_version(
                &runtime,
                &reused_answer,
                Some(&source),
                Some(&source),
            )
            .expect("refresh private context before publishing reused navigation");
            let installed_executable =
                fs::canonicalize(std::env::current_exe().expect("test executable path"))
                    .expect("canonical test executable");
            #[cfg(target_os = "macos")]
            let earlier_reference = ProtectedWorkspaceRoot::inspect(&earlier_presented)
                .and_then(ProtectedWorkspaceRoot::stable_reference)
                .expect("pin reused native workspace");
            #[cfg(not(target_os = "macos"))]
            let earlier_reference = earlier_presented.clone();
            #[cfg(target_os = "macos")]
            let earlier_storage_reference = ProtectedWorkspaceRoot::inspect(&earlier_workspace)
                .and_then(ProtectedWorkspaceRoot::stable_reference)
                .expect("pin reused private storage");
            #[cfg(not(target_os = "macos"))]
            let earlier_storage_reference = earlier_workspace.clone();
            assert_eq!(
                ensure_codex_project_config_at_references(
                    &earlier_presented,
                    &earlier_reference,
                    &earlier_storage_reference,
                    &installed_executable,
                    &endpoint,
                    &earlier_state.installation,
                )
                .expect("recheck exact private Codex context after version switch"),
                CodexProjectConfig::Current,
                "ordinary version navigation exposed an agent-modified Codex configuration",
            );
            let reused = Json::parse(&reused_answer).expect("reused version response");
            assert_eq!(
                reused.get("action").and_then(Json::as_text),
                Some("workspace-version-reopened-existing-copy")
            );
            assert_eq!(
                reused.get("private_store").and_then(Json::as_text),
                earlier_workspace.to_str()
            );
            assert_eq!(reused.get("reused").and_then(Json::as_bool), Some(true));
            assert_eq!(
                fs::read(&source_marker).unwrap(),
                b"mesh.workspace-version-source/1\n",
                "exact reuse never overwrites an interrupted primary marker"
            );
            let recovered_marker =
                earlier_workspace.join(format!(".mesh-source-version.recovered-{first}"));
            assert_eq!(
                fs::read(&recovered_marker).unwrap(),
                format!("mesh.workspace-version-source/1\n{first}\n").as_bytes(),
                "exact reuse publishes the operation-bound recovery hint"
            );
            assert!(
                !application
                    .join("workspace-versions")
                    .join(format!("point-{}-2.mesh", &first[..12]))
                    .exists(),
                "missing marker recovery leaked a duplicate version folder"
            );
            assert_eq!(
                fs::read_link(earlier_presented.join(".codex")).unwrap(),
                std::path::Path::new("../integrations/codex"),
                "reuse removed the verified private Codex integration"
            );

            daemon
                .open_workspace(&current.display().to_string())
                .expect("return after clean reuse");

            // Starting another agent is not ordinary version navigation. A byte-exact clean
            // checkout may still be owned by a long-running process, so this path must allocate a
            // fresh folder instead of using the valid reuse candidate above.
            let current_for_second_agent = daemon
                .workspace_state()
                .expect("current second-agent source");
            let second_agent_answer = open_workspace_version_copy(
                &daemon,
                &versions,
                &first,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &current_for_second_agent.root,
                    digest: &current_for_second_agent.digest,
                    installation: &current_for_second_agent.installation,
                    recent: &recent,
                },
                false,
            )
            .expect("create a fresh checkout for another agent");
            let second_agent =
                Json::parse(&second_agent_answer).expect("second-agent version response");
            assert_eq!(
                second_agent.get("action").and_then(Json::as_text),
                Some("workspace-version-opened-as-copy")
            );
            assert_ne!(
                second_agent.get("reused").and_then(Json::as_bool),
                Some(true),
                "another agent was handed an existing checkout"
            );
            let second_agent_store = PathBuf::from(
                second_agent
                    .get("private_store")
                    .and_then(Json::as_text)
                    .expect("second-agent private store"),
            );
            assert_ne!(second_agent_store, earlier_workspace);
            fs::write(
                second_agent_store
                    .join(mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME)
                    .join("agent-two.txt"),
                "second agent remains here\n",
            )
            .expect("second-agent work");
            daemon
                .open_workspace(&current.display().to_string())
                .expect("return after starting second agent");

            // An agent can reach the private compatibility directory through Mesh's exact
            // `.codex` link. Extra files there must make this checkout inexact: otherwise an
            // agent-controlled rule or tool definition could survive a later "clean" reopen and
            // reach a different agent. Preserve the contaminated checkout and materialize a new
            // saved-point copy instead.
            let injected_codex_file = earlier_presented.join(".codex/agent-injected.toml");
            fs::write(&injected_codex_file, "agent-controlled = true\n")
                .expect("inject private Codex content through the project link");
            let current_after_codex_injection = daemon
                .workspace_state()
                .expect("current after Codex injection");
            let after_codex_injection_answer = open_workspace_version_copy(
                &daemon,
                &versions,
                &first,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &current_after_codex_injection.root,
                    digest: &current_after_codex_injection.digest,
                    installation: &current_after_codex_injection.installation,
                    recent: &recent,
                },
                true,
            )
            .expect("preserve contaminated Codex directory and create a clean version");
            let after_codex_injection = Json::parse(&after_codex_injection_answer)
                .expect("Codex contamination fallback response");
            assert_eq!(
                after_codex_injection.get("action").and_then(Json::as_text),
                Some("workspace-version-opened-as-copy")
            );
            assert_ne!(
                after_codex_injection
                    .get("private_store")
                    .and_then(Json::as_text),
                earlier_workspace.to_str(),
                "agent-controlled private Codex content was reused as an exact checkout"
            );
            let clean_after_codex_store = PathBuf::from(
                after_codex_injection
                    .get("private_store")
                    .and_then(Json::as_text)
                    .expect("clean Codex fallback private store"),
            );
            assert_eq!(
                fs::read_to_string(&injected_codex_file).unwrap(),
                "agent-controlled = true\n",
                "the contaminated checkout was not preserved for inspection"
            );
            fs::write(
                clean_after_codex_store
                    .join(mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME)
                    .join("preserved-after-codex.txt"),
                "keep this fallback distinct\n",
            )
            .expect("make the fallback independently dirty for the remaining reuse cases");
            daemon
                .open_workspace(&current.display().to_string())
                .expect("return after Codex contamination fallback");

            // The exemption is not name based. A replaced .codex link is preserved as possible
            // user work, refuses reuse, and leaves Mesh to create an independent clean checkout.
            let outside_codex = scratch.join("outside-codex");
            fs::create_dir(&outside_codex).expect("outside Codex directory");
            fs::remove_file(earlier_presented.join(".codex")).expect("remove exact Codex link");
            std::os::unix::fs::symlink(&outside_codex, earlier_presented.join(".codex"))
                .expect("replace Codex link");
            let current_after_impostor = daemon.workspace_state().expect("current after impostor");
            let after_impostor_answer = open_workspace_version_copy(
                &daemon,
                &versions,
                &first,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &current_after_impostor.root,
                    digest: &current_after_impostor.digest,
                    installation: &current_after_impostor.installation,
                    recent: &recent,
                },
                true,
            )
            .expect("preserve replaced Codex link and create clean version");
            let after_impostor =
                Json::parse(&after_impostor_answer).expect("impostor fallback response");
            assert_eq!(
                after_impostor.get("action").and_then(Json::as_text),
                Some("workspace-version-opened-as-copy")
            );
            let after_impostor_store = PathBuf::from(
                after_impostor
                    .get("private_store")
                    .and_then(Json::as_text)
                    .expect("clean fallback private store"),
            );
            assert_ne!(after_impostor_store, earlier_workspace);
            assert_eq!(
                fs::canonicalize(earlier_presented.join(".codex")).unwrap(),
                fs::canonicalize(&outside_codex).unwrap(),
                "replaced Codex link was altered during fallback"
            );
            let after_impostor_presented =
                after_impostor_store.join(mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME);

            daemon
                .open_workspace(&current.display().to_string())
                .expect("return after replaced Codex fallback");
            fs::write(
                after_impostor_presented.join("original.txt"),
                "unsaved historical edit\n",
            )
            .expect("dirty earlier checkout");
            let current_after_dirty = daemon.workspace_state().expect("current after dirty copy");
            let fresh_answer = open_workspace_version_copy(
                &daemon,
                &versions,
                &first,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &current_after_dirty.root,
                    digest: &current_after_dirty.digest,
                    installation: &current_after_dirty.installation,
                    recent: &recent,
                },
                true,
            )
            .expect("preserve dirty copy and create fresh earlier version");
            let fresh = Json::parse(&fresh_answer).expect("fresh version response");
            assert_eq!(
                fresh.get("action").and_then(Json::as_text),
                Some("workspace-version-opened-as-copy")
            );
            let fresh_store = PathBuf::from(
                fresh
                    .get("private_store")
                    .and_then(Json::as_text)
                    .expect("fresh private store"),
            );
            assert_ne!(fresh_store, earlier_workspace);
            assert_ne!(fresh_store, after_impostor_store);
            assert_eq!(
                fs::read_to_string(after_impostor_presented.join("original.txt")).unwrap(),
                "unsaved historical edit\n",
                "dirty historical work was overwritten"
            );
            assert_eq!(
                fs::read_to_string(
                    second_agent_store
                        .join(mesh_daemon::workspace::PRESENTED_DIRECTORY_NAME)
                        .join("agent-two.txt")
                )
                .unwrap(),
                "second agent remains here\n",
                "a later version switch reused or rewrote the second agent's folder"
            );
            daemon
                .open_workspace(&current.display().to_string())
                .expect("return after dirty fallback");

            let export = daemon
                .preview_managed_file_export("agent-new.txt", &source)
                .expect("preview export to original");
            daemon
                .export_managed_file(
                    export.path(),
                    &source,
                    export.target_installation(),
                    export.target_parent_installation(),
                    export.target_file_installation(),
                    export.source_version(),
                    export.source_content_digest(),
                    export.source_executable(),
                    export.target_content_digest(),
                    export.target_executable(),
                )
                .expect("explicit export to original");
            assert_eq!(
                fs::read_to_string(source.join("agent-new.txt")).unwrap(),
                "created by local agent\n"
            );
            assert_eq!(
                fs::read_to_string(source.join("notes.txt")).unwrap(),
                "original\n",
                "the tracked edit was not exported implicitly"
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn version_navigation_refuses_a_workspace_changed_after_the_native_open() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-version-navigation-race-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            fs::create_dir_all(&scratch).expect("scratch");
            let application = scratch.join("app");
            fs::create_dir(&application).expect("application data");
            let first =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&scratch.join("first.mesh"))
                    .expect("first workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            let second =
                mesh_daemon::workspace::OpenWorkspace::open_presented(&scratch.join("second.mesh"))
                    .expect("second workspace")
                    .root()
                    .as_path()
                    .to_path_buf();
            let daemon = daemon();
            let first_state = daemon
                .open_workspace(&first.display().to_string())
                .expect("open first workspace");
            let answer = Json::object([(
                "workspace",
                Json::object([
                    ("root", Json::text(&first_state.root)),
                    ("digest", Json::text(&first_state.digest)),
                    ("installation", Json::text(&first_state.installation)),
                ]),
            )])
            .encode();
            daemon
                .open_workspace(&second.display().to_string())
                .expect("concurrent workspace switch");
            let recent = RecentWorkspace::new(application.clone());
            let active = ActiveWorkspaceLink::new(&application);

            let error = commit_opened_workspace_version_navigation(
                &daemon, &recent, &active, &answer, None, None,
            )
            .expect_err("stale native answer must not change navigation");
            assert!(
                error.contains("changed before its native navigation"),
                "{error}"
            );
            assert!(recent.load_entries().unwrap().is_empty());
            assert!(!application.join("native-workspace/current").exists());
            assert_eq!(
                daemon.workspace_state().unwrap().root,
                second.display().to_string()
            );
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn version_navigation_never_reuses_a_folder_handed_to_an_agent() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-agent-handoff-version-reuse-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let source = scratch.join("source");
            let private_workspace = scratch.join("current.mesh");
            let application = scratch.join("app");
            fs::create_dir_all(&source).expect("source");
            fs::create_dir(&application).expect("application data");
            fs::write(source.join("notes.txt"), "saved point\n").expect("source file");

            let daemon = daemon();
            let preview = daemon
                .preview_folder_import(&source.display().to_string())
                .expect("preview import");
            let summary = preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("import summary");
            let imported = daemon
                .confirm_folder_import(
                    &source.display().to_string(),
                    &private_workspace.display().to_string(),
                    summary,
                )
                .expect("confirm import");
            let current = PathBuf::from(
                imported
                    .get("destination")
                    .and_then(Json::as_text)
                    .expect("presented workspace"),
            );
            let current_state = daemon.workspace_state().expect("current state");
            let operation = current_state.workspace_versions[0].operation().to_string();
            let recent = RecentWorkspace::new(application.clone());
            let active = ActiveWorkspaceLink::new(&application);
            remember_current_workspace_navigation_with_project_root(
                &daemon,
                &recent,
                &active,
                &current_state,
                Some(&source),
                Some(&source),
                None,
            )
            .expect("remember imported workspace");
            let versions = VersionWorkspaceDirectory::new(&application);

            let first_answer = open_workspace_version_copy(
                &daemon,
                &versions,
                &operation,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &current_state.root,
                    digest: &current_state.digest,
                    installation: &current_state.installation,
                    recent: &recent,
                },
                false,
            )
            .expect("create first agent folder");
            commit_opened_workspace_version_navigation(
                &daemon,
                &recent,
                &active,
                &first_answer,
                Some(&source),
                Some(&source),
            )
            .expect("remember agent folder");
            let first = Json::parse(&first_answer).expect("first answer");
            let first_store = PathBuf::from(
                first
                    .get("private_store")
                    .and_then(Json::as_text)
                    .expect("first private store"),
            );
            let first_presented = PathBuf::from(
                first
                    .get("destination")
                    .and_then(Json::as_text)
                    .expect("first presented folder"),
            );
            let first_state = daemon.workspace_state().expect("first agent state");
            recent
                .record_agent_handoff(
                    &first_presented,
                    &first_state.installation,
                    &ProtectedWorkspaceRoot::inspect(&first_presented)
                        .expect("first agent directory identity")
                        .directory_token(),
                )
                .expect("record active agent handoff");

            let returned = daemon
                .open_workspace(&current.display().to_string())
                .expect("return to imported workspace");
            let second_answer = open_workspace_version_copy(
                &daemon,
                &versions,
                &operation,
                None,
                Some(&source),
                WorkspaceVersionSource {
                    root: &returned.root,
                    digest: &returned.digest,
                    installation: &returned.installation,
                    recent: &recent,
                },
                true,
            )
            .expect("open saved point without reusing active agent folder");
            let second = Json::parse(&second_answer).expect("second answer");
            let second_presented = PathBuf::from(
                second
                    .get("destination")
                    .and_then(Json::as_text)
                    .expect("second presented folder"),
            );
            assert_eq!(
                second.get("action").and_then(Json::as_text),
                Some("workspace-version-opened-as-copy")
            );
            assert_ne!(
                second.get("private_store").and_then(Json::as_text),
                first_store.to_str(),
                "ordinary navigation reused a folder still handed to an agent"
            );
            assert_eq!(
                fs::read_to_string(first_presented.join("notes.txt")).unwrap(),
                "saved point\n",
                "the active agent folder was not preserved"
            );
            commit_opened_workspace_version_navigation(
                &daemon,
                &recent,
                &active,
                &second_answer,
                Some(&source),
                Some(&source),
            )
            .expect("retarget navigation to independent second folder");
            let stable = application.join("native-workspace/current");
            assert_eq!(
                fs::read_link(&stable).expect("read retargeted stable link"),
                fs::canonicalize(&second_presented).expect("canonical second folder"),
                "the stable human folder did not move to the newly opened saved point"
            );
            fs::write(first_presented.join("agent-still-running.txt"), "agent\n")
                .expect("continue agent write through pinned physical folder");
            fs::write(stable.join("human-on-selected-version.txt"), "human\n")
                .expect("write through retargeted stable folder");
            assert!(first_presented.join("agent-still-running.txt").is_file());
            assert!(second_presented
                .join("human-on-selected-version.txt")
                .is_file());
            assert!(
                !first_presented
                    .join("human-on-selected-version.txt")
                    .exists(),
                "the retargeted human write reached the pinned agent folder"
            );
            assert!(
                !second_presented.join("agent-still-running.txt").exists(),
                "the pinned agent write reached the selected human folder"
            );
            let recorded = recent.load_entries().expect("reload handoff record");
            let canonical_first_presented =
                fs::canonicalize(&first_presented).expect("canonical first presented folder");
            assert!(recorded.iter().any(|entry| {
                entry.path() == canonical_first_presented
                    && entry.agent_handoff_installation() == Some(first_state.installation.as_str())
            }));
            assert!(recorded.iter().any(|entry| {
                entry.path()
                    == fs::canonicalize(&second_presented).expect("canonical current folder")
                    && entry.agent_handoff_installation().is_none()
            }));
            let _ = fs::remove_dir_all(scratch);
        }

        #[test]
        fn stable_navigation_survives_forgetting_but_deactivates_after_rollback() {
            let scratch = std::env::temp_dir().join(format!(
                "mesh-desktop-native-rollback-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&scratch);
            let source = scratch.join("source");
            let private_workspace = scratch.join("managed.mesh");
            fs::create_dir_all(&source).expect("source");
            fs::create_dir(scratch.join("app")).expect("application data");
            fs::write(source.join("notes.txt"), "original\n").expect("source file");
            let daemon = daemon();
            let preview = daemon
                .preview_folder_import(&source.display().to_string())
                .expect("preview import");
            let summary = preview
                .get("summary")
                .and_then(Json::as_text)
                .expect("import summary");
            let imported = daemon
                .confirm_folder_import(
                    &source.display().to_string(),
                    &private_workspace.display().to_string(),
                    summary,
                )
                .expect("confirm import");
            let presented = imported
                .get("destination")
                .and_then(Json::as_text)
                .expect("presented folder")
                .to_owned();
            let recent = RecentWorkspace::new(scratch.join("app"));
            let remembered = recent
                .remember(std::path::Path::new(&presented))
                .expect("remember current workspace");
            let active = ActiveWorkspaceLink::new(&scratch.join("app"));
            let stable = reconcile_active_native_folder(&daemon, &active)
                .expect("reconcile current workspace")
                .expect("presented native folder");
            assert!(stable.is_symlink());

            // Removing a recent-history hint does not close the current workspace. Reconciliation
            // must therefore preserve its useful stable native path.
            let forgotten = forget_workspace_navigation(&daemon, &recent, &active, &remembered)
                .expect("forget current navigation hint");
            assert!(forgotten.workspaces.is_empty());
            assert_eq!(forgotten.active_folder.as_deref(), stable.to_str());

            let rolled_back = daemon
                .rollback_folder_import(&presented)
                .expect("rollback current import");
            let canonical_deleted_root = rolled_back
                .get("workspace_root")
                .and_then(Json::as_text)
                .expect("canonical deleted workspace root");
            assert_eq!(canonical_deleted_root, remembered.to_str().unwrap());
            let after_rollback = forget_workspace_navigation(
                &daemon,
                &recent,
                &active,
                std::path::Path::new(canonical_deleted_root),
            )
            .expect("idempotently forget rolled-back workspace");
            assert!(after_rollback.active_folder.is_none());
            assert!(matches!(
                fs::symlink_metadata(&stable),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            ));
            assert!(!private_workspace.exists());
            assert_eq!(
                fs::read_to_string(source.join("notes.txt")).unwrap(),
                "original\n"
            );
            let _ = fs::remove_dir_all(scratch);
        }
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn ignore_appkit_persistent_window_state() {
    use std::ffi::{c_char, c_void};

    const UTF8: u32 = 0x0800_0100;
    const KEY: &[u8] = b"ApplePersistenceIgnoreState\0";

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFAllocatorDefault: *const c_void;
        static kCFBooleanTrue: *const c_void;
        static kCFPreferencesCurrentApplication: *const c_void;

        fn CFStringCreateWithCString(
            allocator: *const c_void,
            value: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFPreferencesSetAppValue(
            key: *const c_void,
            value: *const c_void,
            application_id: *const c_void,
        );
        fn CFPreferencesAppSynchronize(application_id: *const c_void) -> bool;
        fn CFRelease(value: *const c_void);
    }

    // AppKit consults this preference while handling the launch Apple event, before Tauri invokes
    // its setup callback. Set it before the event loop starts so stale crash-window state can
    // never interpose a modal dialog ahead of Mesh's own verified recent-workspace restoration.
    // SAFETY: the CoreFoundation constants are process-lifetime objects, KEY is NUL-terminated,
    // and the one created CFString is released exactly once after both calls return.
    unsafe {
        let key =
            CFStringCreateWithCString(kCFAllocatorDefault, KEY.as_ptr().cast::<c_char>(), UTF8);
        assert!(!key.is_null(), "could not create the AppKit preference key");
        CFPreferencesSetAppValue(key, kCFBooleanTrue, kCFPreferencesCurrentApplication);
        let _ = CFPreferencesAppSynchronize(kCFPreferencesCurrentApplication);
        CFRelease(key);
    }
}

#[cfg(unix)]
#[derive(Debug, Eq, PartialEq)]
struct MeshMcpInvocation {
    endpoint: std::path::PathBuf,
    root: std::path::PathBuf,
    installation: String,
}

#[cfg(unix)]
fn parse_mesh_mcp_invocation(args: &[String]) -> Result<Option<MeshMcpInvocation>, String> {
    if args.first().map(String::as_str) != Some("--mesh-mcp") {
        return Ok(None);
    }
    let mut endpoint = None;
    let mut root = None;
    let mut installation = None;
    let mut index = 1;
    while index < args.len() {
        let option = args[index].as_str();
        let value = match args.get(index + 1) {
            Some(value) if !value.is_empty() && !value.starts_with('-') => value.clone(),
            _ => return Err(format!("{option} requires a non-empty value")),
        };
        let destination = match option {
            "--endpoint" => &mut endpoint,
            "--expected-workspace-root" => &mut root,
            "--expected-workspace-installation" => &mut installation,
            _ => return Err(format!("unknown Mesh MCP option `{option}`")),
        };
        if destination.replace(value).is_some() {
            return Err(format!("{option} may be supplied only once"));
        }
        index += 2;
    }
    Ok(Some(MeshMcpInvocation {
        endpoint: endpoint
            .map(std::path::PathBuf::from)
            .ok_or("--endpoint is required")?,
        root: root
            .map(std::path::PathBuf::from)
            .ok_or("--expected-workspace-root is required")?,
        installation: installation.ok_or("--expected-workspace-installation is required")?,
    }))
}

#[cfg(unix)]
fn run_mesh_mcp_if_requested() -> Option<Result<(), String>> {
    use mesh_mcp::{serve, DaemonWorkspaceState, ExpectedWorkspace};
    use std::io::{self, BufReader};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let invocation = match parse_mesh_mcp_invocation(&args) {
        Ok(Some(invocation)) => invocation,
        Ok(None) => return None,
        Err(problem) => return Some(Err(problem)),
    };
    let result = {
        let expected = ExpectedWorkspace::new(invocation.root, invocation.installation);
        let provider = DesktopMcpWorkspaceState::new(DaemonWorkspaceState::bound(
            invocation.endpoint,
            expected,
        ));
        serve(
            BufReader::new(io::stdin().lock()),
            io::stdout().lock(),
            &provider,
        )
    };
    Some(result)
}

/// This mode never falls back to the selected workspace or the graphical application.
#[cfg(unix)]
fn parse_mesh_fleet_mcp_invocation(args: &[String]) -> Result<Option<std::path::PathBuf>, String> {
    if args.first().map(String::as_str) != Some("--mesh-fleet-mcp") {
        return Ok(None);
    }
    if args.len() != 3
        || args[1] != "--endpoint"
        || !std::path::Path::new(&args[2]).is_absolute()
        || args[2].contains('\0')
    {
        return Err("Usage: Mesh --mesh-fleet-mcp --endpoint <absolute-local-socket>".into());
    }
    Ok(Some(std::path::PathBuf::from(&args[2])))
}

#[cfg(unix)]
fn desktop_fleet_provider(
    endpoint: std::path::PathBuf,
    objective: Option<String>,
    credential: Option<String>,
) -> Result<mesh_mcp::DaemonWorkspaceState, String> {
    let (Some(objective), Some(credential)) = (objective, credential) else {
        return Err("A complete native fleet session is required".into());
    };
    mesh_mcp::DaemonWorkspaceState::fleet(endpoint, objective, credential)
}

#[cfg(unix)]
fn run_mesh_fleet_mcp_if_requested() -> Option<Result<(), String>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let endpoint = match parse_mesh_fleet_mcp_invocation(&args) {
        Ok(Some(endpoint)) => endpoint,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    Some((|| {
        let provider = DesktopMcpWorkspaceState::new(desktop_fleet_provider(
            endpoint,
            std::env::var("MESH_FLEET_OBJECTIVE").ok(),
            std::env::var("MESH_FLEET_CREDENTIAL").ok(),
        )?);
        mesh_mcp::serve(
            std::io::BufReader::new(std::io::stdin().lock()),
            std::io::stdout().lock(),
            &provider,
        )
    })())
}

/// Attach the exact desktop build identity to the daemon projection handed to an agent.
///
/// The MCP package version describes the wire producer and intentionally remains `0.0.0` during
/// the alpha. It is not enough to diagnose which ad-hoc application archive launched a session.
/// Keep the daemon's workspace object intact and append the build identity embedded by the
/// desktop build script; a future daemon field with the same name fails closed instead of being
/// shadowed by a duplicate JSON key.
#[cfg(unix)]
struct DesktopMcpWorkspaceState<P> {
    inner: P,
}

#[cfg(unix)]
impl<P> DesktopMcpWorkspaceState<P> {
    fn new(inner: P) -> Self {
        Self { inner }
    }
}

#[cfg(unix)]
impl<P: mesh_mcp::WorkspaceStateProvider> mesh_mcp::WorkspaceStateProvider
    for DesktopMcpWorkspaceState<P>
{
    fn fleet_enabled(&self) -> bool {
        self.inner.fleet_enabled()
    }

    fn fleet_call(
        &self,
        action: &str,
        arguments: &mesh_mcp::json::Json,
    ) -> Result<mesh_mcp::json::Json, String> {
        let result = self.inner.fleet_call(action, arguments)?;
        if action == "context" {
            desktop_agent_build_identity(result)
        } else {
            Ok(result)
        }
    }

    fn workspace_state(&self) -> Result<mesh_mcp::json::Json, String> {
        desktop_agent_build_identity(self.inner.workspace_state()?)
    }
}

#[cfg(unix)]
fn desktop_agent_build_identity(
    value: mesh_mcp::json::Json,
) -> Result<mesh_mcp::json::Json, String> {
    use mesh_mcp::json::Json;

    let Json::Object(mut fields) = value else {
        return Err("Mesh workspace state was not an object".to_owned());
    };
    for name in ["mesh_desktop_build_revision", "mesh_desktop_build_exact"] {
        if fields.iter().any(|(field, _)| field == name) {
            return Err(format!(
                "Mesh workspace state unexpectedly supplied reserved agent field `{name}`"
            ));
        }
    }
    fields.push((
        "mesh_desktop_build_revision".to_owned(),
        Json::text(env!("MESH_BUILD_REVISION")),
    ));
    fields.push((
        "mesh_desktop_build_exact".to_owned(),
        Json::Bool(env!("MESH_BUILD_REVISION") != "development"),
    ));
    Ok(Json::Object(fields))
}

#[cfg(unix)]
fn finish_mesh_mcp_mode(result: Result<(), String>) {
    if let Err(problem) = result {
        eprintln!("mesh-mcp: {problem}");
        std::process::exit(1);
    }
}

#[cfg(all(test, unix))]
mod mesh_mcp_mode_tests {
    use super::*;
    use mesh_mcp::WorkspaceStateProvider as _;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn exact_invocation_is_parsed_and_other_desktop_args_are_ignored() {
        assert_eq!(
            parse_mesh_mcp_invocation(&args(&["ordinary"])).unwrap(),
            None
        );
        let parsed = parse_mesh_mcp_invocation(&args(&[
            "--mesh-mcp",
            "--endpoint",
            "/private/run/mesh.sock",
            "--expected-workspace-root",
            "/native/version",
            "--expected-workspace-installation",
            "installation",
        ]))
        .expect("parse")
        .expect("MCP mode");
        assert_eq!(
            parsed.endpoint,
            std::path::Path::new("/private/run/mesh.sock")
        );
        assert_eq!(parsed.root, std::path::Path::new("/native/version"));
        assert_eq!(parsed.installation, "installation");
    }

    #[test]
    fn missing_repeated_unknown_and_option_values_are_refused() {
        for values in [
            vec!["--mesh-mcp"],
            vec!["--mesh-mcp", "--endpoint", "--help"],
            vec!["--mesh-mcp", "--unknown", "value"],
            vec!["--mesh-mcp", "--endpoint", "/one", "--endpoint", "/two"],
        ] {
            assert!(parse_mesh_mcp_invocation(&args(&values)).is_err());
        }
    }

    #[test]
    fn fleet_mode_requires_exact_arguments_and_a_complete_native_session() {
        assert!(parse_mesh_fleet_mcp_invocation(&args(&["--mesh-mcp"]))
            .unwrap()
            .is_none());
        assert_eq!(
            parse_mesh_fleet_mcp_invocation(&args(&[
                "--mesh-fleet-mcp",
                "--endpoint",
                "/private/run/fleet.sock"
            ]))
            .unwrap(),
            Some(std::path::PathBuf::from("/private/run/fleet.sock"))
        );
        for values in [
            vec!["--mesh-fleet-mcp"],
            vec!["--mesh-fleet-mcp", "--endpoint", "relative"],
            vec![
                "--mesh-fleet-mcp",
                "--endpoint",
                "/one",
                "--endpoint",
                "/two",
            ],
            vec!["--mesh-fleet-mcp", "--credential", "not-on-command-line"],
            vec![
                "--mesh-fleet-mcp",
                "--endpoint",
                "/one",
                "--expected-workspace-root",
                "/other",
            ],
        ] {
            assert!(parse_mesh_fleet_mcp_invocation(&args(&values)).is_err());
        }
        let endpoint = std::path::PathBuf::from("/private/run/fleet.sock");
        for (objective, credential) in [
            (None, None),
            (Some("objective".into()), None),
            (None, Some("a".repeat(64))),
            (Some("invalid/name".into()), Some("a".repeat(64))),
            (
                Some("objective".into()),
                Some("secret-invalid-token".into()),
            ),
        ] {
            let error =
                desktop_fleet_provider(endpoint.clone(), objective, credential).unwrap_err();
            assert!(!error.contains("secret-invalid-token"));
        }
        let provider =
            desktop_fleet_provider(endpoint, Some("objective".into()), Some("a".repeat(64)))
                .unwrap();
        assert!(DesktopMcpWorkspaceState::new(provider).fleet_enabled());
    }

    struct FixedFleetState {
        reserved_field: Option<&'static str>,
    }
    impl mesh_mcp::WorkspaceStateProvider for FixedFleetState {
        fn workspace_state(&self) -> Result<mesh_mcp::json::Json, String> {
            Err("no selected workspace".into())
        }
        fn fleet_enabled(&self) -> bool {
            true
        }
        fn fleet_call(
            &self,
            action: &str,
            arguments: &mesh_mcp::json::Json,
        ) -> Result<mesh_mcp::json::Json, String> {
            if action == "context" {
                let mut fields = vec![(
                    "lane".to_owned(),
                    mesh_mcp::json::Json::text("assigned-lane"),
                )];
                if let Some(name) = self.reserved_field {
                    fields.push((
                        name.to_owned(),
                        mesh_mcp::json::Json::text("untrusted-build"),
                    ));
                }
                Ok(mesh_mcp::json::Json::Object(fields))
            } else {
                Ok(arguments.clone())
            }
        }
    }

    #[test]
    fn fleet_wrapper_keeps_scoped_calls_and_exact_build_identity_without_selection() {
        use mesh_mcp::json::Json;
        let provider = DesktopMcpWorkspaceState::new(FixedFleetState {
            reserved_field: None,
        });
        assert!(provider.fleet_enabled());
        assert!(provider.workspace_state().is_err());
        let context = provider
            .fleet_call("context", &Json::empty_object())
            .unwrap();
        assert_eq!(context.get("lane"), Some(&Json::text("assigned-lane")));
        assert_eq!(
            context.get("mesh_desktop_build_revision"),
            Some(&Json::text(env!("MESH_BUILD_REVISION")))
        );
        let saved = Json::object([("request", Json::text("stable-request"))]);
        assert_eq!(provider.fleet_call("checkpoint", &saved).unwrap(), saved);
        assert!(!DesktopMcpWorkspaceState::new(FixedWorkspaceState {
            reserved_field: false
        })
        .fleet_enabled());
    }

    #[test]
    fn fleet_context_refuses_reserved_build_fields_without_using_selected_workspace() {
        use mesh_mcp::json::Json;
        for field in ["mesh_desktop_build_revision", "mesh_desktop_build_exact"] {
            let provider = DesktopMcpWorkspaceState::new(FixedFleetState {
                reserved_field: Some(field),
            });
            let error = provider
                .fleet_call("context", &Json::empty_object())
                .unwrap_err();
            assert!(error.contains("reserved agent field"));
            assert!(!error.contains("untrusted-build"));
            assert!(provider.workspace_state().is_err());
        }
        assert!(desktop_agent_build_identity(Json::Null).is_err());
    }

    struct FixedWorkspaceState {
        reserved_field: bool,
    }

    impl mesh_mcp::WorkspaceStateProvider for FixedWorkspaceState {
        fn workspace_state(&self) -> Result<mesh_mcp::json::Json, String> {
            use mesh_mcp::json::Json;

            let mut fields = vec![("root".to_owned(), Json::text("/native/version"))];
            if self.reserved_field {
                fields.push((
                    "mesh_desktop_build_revision".to_owned(),
                    Json::text("forged"),
                ));
            }
            Ok(Json::Object(fields))
        }
    }

    #[test]
    fn agent_state_carries_one_exact_desktop_build_identity() {
        let state = DesktopMcpWorkspaceState::new(FixedWorkspaceState {
            reserved_field: false,
        })
        .workspace_state()
        .expect("desktop agent state");
        assert_eq!(
            state
                .get("mesh_desktop_build_revision")
                .and_then(mesh_mcp::json::Json::as_text),
            Some(env!("MESH_BUILD_REVISION"))
        );
        assert_eq!(
            state
                .get("mesh_desktop_build_exact")
                .and_then(mesh_mcp::json::Json::as_bool),
            Some(env!("MESH_BUILD_REVISION") != "development")
        );

        let problem = DesktopMcpWorkspaceState::new(FixedWorkspaceState {
            reserved_field: true,
        })
        .workspace_state()
        .expect_err("reserved daemon field must be rejected");
        assert!(problem.contains("reserved agent field"), "{problem}");
    }
}

#[cfg(target_os = "macos")]
fn main() {
    if build_identity::run_if_requested() {
        return;
    }
    if let Some(result) = worker_installation::run_if_requested() {
        if let Err(problem) = result {
            eprintln!("Mesh worker: {problem}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = attachment_capture::run_if_requested() {
        if let Err(problem) = result {
            eprintln!("Mesh attachment: {problem}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = run_mesh_fleet_mcp_if_requested() {
        finish_mesh_mcp_mode(result);
        return;
    }
    if let Some(result) = run_mesh_mcp_if_requested() {
        finish_mesh_mcp_mode(result);
        return;
    }
    ignore_appkit_persistent_window_state();
    desktop::run();
}

#[cfg(all(unix, not(target_os = "macos")))]
fn main() {
    if build_identity::run_if_requested() {
        return;
    }
    if let Some(result) = worker_installation::run_if_requested() {
        if let Err(problem) = result {
            eprintln!("Mesh worker: {problem}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = attachment_capture::run_if_requested() {
        if let Err(problem) = result {
            eprintln!("Mesh attachment: {problem}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(result) = run_mesh_fleet_mcp_if_requested() {
        finish_mesh_mcp_mode(result);
        return;
    }
    if let Some(result) = run_mesh_mcp_if_requested() {
        finish_mesh_mcp_mode(result);
        return;
    }
    desktop::run();
}
