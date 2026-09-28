//! Native desktop attachment sessions. The renderer supplies a source selection, never store paths.

use crate::attachment_capture::NativeCaptureSigner;
use mesh_daemon::ipc::Json;
use mesh_daemon::project_attachment::{
    AttachmentCaptureService, AttachmentPinState, AttachmentStorage, CaptureOutcome, CapturePhase,
    CaptureSchedule, CaptureStatus, FleetPinState, NativeSignalState, ProvisionedAttachment,
};
use std::collections::BTreeMap;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_PROJECTS: usize = 32;
const UNAVAILABLE: &str = "Attachment storage is unavailable or needs reconciliation";

pub struct AttachmentHost {
    storage_path: PathBuf,
    #[cfg(target_os = "macos")]
    fleet_path: PathBuf,
    #[cfg(target_os = "macos")]
    fleets: Mutex<Option<mesh_daemon::fleet::catalog::NativeFleetDirectory>>,
    state: Mutex<HostState>,
}
#[derive(Default)]
struct HostState {
    storage: Option<AttachmentStorage>,
    projects: BTreeMap<String, Project>,
    generation: u64,
}
struct Project {
    detached: bool,
    source: PathBuf,
    generation: u64,
    service: Option<AttachmentCaptureService>,
    history: Option<ProvisionedAttachment>,
    recovered: CaptureStatus,
    recovery: Option<&'static str>,
    lane: Json,
}
impl Project {
    fn status(&self) -> CaptureStatus {
        self.service
            .as_ref()
            .map_or_else(|| self.recovered.clone(), AttachmentCaptureService::status)
    }
    fn projection(&self, id: &str) -> Json {
        Json::object([
            ("detached", Json::Bool(self.detached)),
            ("id", Json::text(id)),
            ("generation", Json::text(self.generation.to_string())),
            ("root", Json::text(self.source.to_string_lossy())),
            ("capture", self.status().to_json()),
            ("recovery", self.recovery.map_or(Json::Null, Json::text)),
            ("lane", self.lane.clone()),
        ])
    }
}
impl AttachmentHost {
    pub fn new(application_data: &Path) -> Self {
        Self {
            storage_path: application_data.join("attached-projects"),
            #[cfg(target_os = "macos")]
            fleet_path: application_data.join("fleets"),
            #[cfg(target_os = "macos")]
            fleets: Mutex::new(None),
            state: Mutex::new(HostState::default()),
        }
    }

    fn initialize(&self, state: &mut HostState, create: bool) -> Result<(), String> {
        if state.storage.is_some() {
            return Ok(());
        }
        if !create {
            match std::fs::symlink_metadata(&self.storage_path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(_) => return Err(UNAVAILABLE.into()),
            }
        }
        if create {
            match DirBuilder::new().mode(0o700).create(&self.storage_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(UNAVAILABLE.into()),
            }
        }
        let storage = AttachmentStorage::open(&self.storage_path).map_err(|_| UNAVAILABLE)?;
        let registrations = storage.registrations().map_err(|_| UNAVAILABLE)?;
        if registrations.len() > MAX_PROJECTS {
            return Err("The desktop attachment limit has been reached".into());
        }
        let lane_root = self
            .storage_path
            .canonicalize()
            .map_err(|_| UNAVAILABLE)?
            .join("work-lanes");
        let mut recovered = BTreeMap::new();
        let mut generation = state.generation;
        for registration in registrations {
            generation = generation.checked_add(1).ok_or(UNAVAILABLE)?;
            let history = storage.reopen(registration.id()).ok();
            let versions = history
                .as_ref()
                .and_then(|history| history.saved_versions().ok());
            let ready = versions.is_some();
            let saved = versions.and_then(|versions| versions.last().copied());
            let mut status = stopped_status();
            status.saved_version = saved;
            recovered.insert(
                registration.id().to_owned(),
                Project {
                    lane: history.as_ref().map_or_else(
                        || {
                            if registration.root().starts_with(&lane_root) {
                                unavailable_lane()
                            } else {
                                Json::Null
                            }
                        },
                        |history| lane_origin(&storage, history),
                    ),
                    detached: registration.detached(),
                    source: registration.root().to_owned(),
                    generation,
                    service: None,
                    history,
                    recovered: status,
                    recovery: Some(if ready {
                        "restored-stopped"
                    } else {
                        "unavailable"
                    }),
                },
            );
        }
        state.projects = recovered;
        state.generation = generation;
        state.storage = Some(storage);
        Ok(())
    }

    pub fn attach(&self, source: &Path) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        self.initialize(&mut state, true)?;
        // Serialize admission and worker creation so simultaneous selections cannot duplicate workers.
        if state.projects.len() >= MAX_PROJECTS
            && !state
                .projects
                .values()
                .any(|project| project.source == source)
        {
            return Err("The desktop attachment limit has been reached".into());
        }
        let provisioned = state
            .storage
            .as_ref()
            .ok_or(UNAVAILABLE)?
            .provision(source)
            .map_err(|_| UNAVAILABLE)?;
        let id = provisioned.id().to_owned();
        if let Some(project) = state.projects.get(&id) {
            // Reselecting never silently resumes an explicitly stopped or failed session.
            return Ok(project.projection(&id).encode());
        }
        let source = provisioned.project().root().to_owned();
        let generation = state.generation.checked_add(1).ok_or(UNAVAILABLE)?;
        let service = provisioned
            .start_capture(NativeCaptureSigner::generate()?, CaptureSchedule::default())
            .map_err(|_| UNAVAILABLE)?;
        state.generation = generation;
        let project = Project {
            lane: lane_origin(state.storage.as_ref().ok_or(UNAVAILABLE)?, &provisioned),
            detached: false,
            source,
            generation,
            service: Some(service),
            history: Some(provisioned),
            recovered: stopped_status(),
            recovery: None,
        };
        let response = project.projection(&id).encode();
        state.projects.insert(id, project);
        Ok(response)
    }

    pub fn open_version_lane(
        &self,
        id: &str,
        version: &str,
        request: &str,
    ) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        self.initialize(&mut state, false)?;
        let source = state
            .projects
            .get(id)
            .ok_or("This attachment is not open")?
            .history
            .clone()
            .ok_or("Saved history is unavailable")?;
        // Reserve room before allocating. A retry for a registered child is still permitted.
        if state.projects.len() >= MAX_PROJECTS {
            let known = state.projects.values().any(|project| {
                project.lane.get("source_project") == Some(&Json::text(id))
                    && project.lane.get("request") == Some(&Json::text(request))
            });
            if !known {
                return Err("The desktop attachment limit has been reached".into());
            }
        }
        let storage = state.storage.as_ref().ok_or(UNAVAILABLE)?;
        let child = storage.open_version_lane(&source, version, request,
            mesh_daemon::project_attachment::ObservationLimits::default())
            .map_err(|_| "This lane could not be confirmed. Retry the same request; partial work is retained and will not be overwritten.")?;
        let child_id = child.id().to_owned();
        let lane = lane_origin(storage, &child);
        if !state.projects.contains_key(&child_id) {
            let generation = state.generation.checked_add(1).ok_or(UNAVAILABLE)?;
            let service = child
                .start_capture(NativeCaptureSigner::generate()?, CaptureSchedule::default())
                .map_err(|_| {
                    "The lane is retained, but capture could not start. Retry the same request."
                })?;
            state.generation = generation;
            state.projects.insert(
                child_id.clone(),
                Project {
                    detached: false,
                    source: child.project().root().to_owned(),
                    generation,
                    service: Some(service),
                    history: Some(child),
                    recovered: stopped_status(),
                    recovery: None,
                    lane,
                },
            );
        }
        Ok(Json::object([
            ("schema", Json::text("mesh.desktop-attachment-lane/v1")),
            ("source_project", Json::text(id)),
            ("source_version", Json::text(version)),
            ("request", Json::text(request)),
            ("project", Json::text(child_id)),
        ])
        .encode())
    }

    #[cfg(target_os = "macos")]
    fn with_fleets<T>(
        &self,
        create: bool,
        action: impl FnOnce(
            Option<&mesh_daemon::fleet::catalog::NativeFleetDirectory>,
        ) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut held = self
            .fleets
            .lock()
            .map_err(|_| "Fleet storage is unavailable")?;
        if held.is_none() {
            if !create {
                match std::fs::symlink_metadata(&self.fleet_path) {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return action(None)
                    }
                    Err(_) => return Err("Fleet storage is unavailable".into()),
                }
            }
            if create {
                match DirBuilder::new().mode(0o700).create(&self.fleet_path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(_) => return Err("Fleet storage is unavailable".into()),
                }
            }
            *held = Some(
                mesh_daemon::fleet::catalog::NativeFleetDirectory::open(
                    &self.fleet_path,
                    mesh_daemon::TrustedReviewers::default(),
                    mesh_daemon::CheckpointRuntimeParameters {
                        idle_interval: Some(std::time::Duration::from_millis(50)),
                        maximum_uncheckpointed_bytes: Some(65_536),
                        maximum_uncheckpointed_interval: Some(std::time::Duration::from_secs(2)),
                    },
                )
                .map_err(|_| "Fleet storage is unavailable or owned by another host")?,
            );
        }
        action(held.as_ref())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn current_fleet(
        &self,
        _objective: &str,
    ) -> Result<std::sync::Arc<mesh_daemon::fleet::service::FleetService>, String> {
        Err("Native fleet execution is unavailable on this platform".into())
    }

    #[cfg(target_os = "macos")]
    pub fn current_fleet(
        &self,
        objective: &str,
    ) -> Result<std::sync::Arc<mesh_daemon::fleet::service::FleetService>, String> {
        self.with_fleets(false, |directory| {
            directory
                .ok_or("Fleet is not available in this app session")?
                .current_service(objective)
                .map_err(|_| "Fleet needs recovery before execution".into())
        })
    }

    /// Saved history access is independent of execution ownership after restart.
    pub fn fleet_history(
        &self,
        objective: &str,
    ) -> Result<mesh_daemon::fleet::service::FleetHistory, String> {
        #[cfg(target_os = "macos")]
        {
            self.with_fleets(false, |directory| {
                directory
                    .ok_or("Fleet history is unavailable")?
                    .history(objective)
                    .map_err(|_| "Fleet history could not be verified".into())
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = objective;
            Err("Native fleet history is unavailable on this platform".into())
        }
    }

    pub fn fleets(&self) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        {
            self.with_fleets(false, |directory| match directory {
                Some(directory) => directory
                    .snapshot()
                    .map(|value| value.encode())
                    .map_err(|_| {
                        "Fleet status is unavailable; retained work needs reconciliation".into()
                    }),
                None => Ok(Json::object([
                    ("schema", Json::text("mesh.native-fleets/v1")),
                    ("fleets", Json::Array(vec![])),
                ])
                .encode()),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err("Native fleet storage is unavailable on this platform".into())
        }
    }

    pub fn provision_fleet(
        &self,
        id: &str,
        request: &str,
        goal: &str,
        version: &str,
        limits_json: &str,
    ) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        {
            if limits_json.len() > 256 {
                return Err("Invalid fleet limits".into());
            }
            let value = Json::parse(limits_json).map_err(|_| "Invalid fleet limits")?;
            let Json::Object(fields) = &value else {
                return Err("Invalid fleet limits".into());
            };
            if fields.len() != 4
                || fields.iter().any(|(key, _)| {
                    !["lanes", "concurrency", "depth", "retries"].contains(&key.as_str())
                })
            {
                return Err("Invalid fleet limits".into());
            }
            let number = |name| {
                value
                    .get(name)
                    .and_then(Json::as_u64)
                    .ok_or("Invalid fleet limits")
            };
            let input = mesh_daemon::fleet::catalog::AttachedFleetRequest::new(
                request,
                goal,
                version,
                mesh_daemon::fleet::Limits {
                    lanes: number("lanes")?,
                    concurrency: number("concurrency")?,
                    depth: number("depth")?,
                    retries: number("retries")?,
                },
            )
            .map_err(|_| "Invalid fleet request")?;
            let source = self.review_history(id)?;
            self.with_fleets(true, |directory| {
                let service = directory.ok_or("Fleet storage is unavailable")?.create_attached(&source, &input)
                    .map_err(|_| "Fleet allocation could not be confirmed. Retained work requires reconciliation before a different attempt.")?;
                let objective = service.objective().map_err(|_| "Fleet identity is unavailable")?;
                Ok(Json::object([
                    ("schema",Json::text("mesh.desktop-attached-fleet/v1")), ("project",Json::text(id)),
                    ("request",Json::text(request)), ("objective",Json::text(objective)), ("started",Json::Bool(false)),
                ]).encode())
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (id, request, goal, version, limits_json);
            Err("Native fleet storage is unavailable on this platform".into())
        }
    }

    pub fn projects(&self) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        self.initialize(&mut state, false)?;
        Ok(Json::object([
            ("schema", Json::text("mesh.desktop-attachments/v1")),
            (
                "projects",
                Json::Array(
                    state
                        .projects
                        .iter()
                        .map(|(id, project)| project.projection(id))
                        .collect(),
                ),
            ),
        ])
        .encode())
    }

    pub fn versions(&self, id: &str, before: Option<&str>) -> Result<String, String> {
        // Retain authority, then release the registry lock before journal verification or disk IO.
        let history = self
            .state
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .projects
            .get(id)
            .ok_or("This attachment is not open in this desktop session")?
            .history
            .clone()
            .ok_or("The original project or its saved history is unavailable")?;
        let versions = history
            .saved_versions()
            .map_err(|_| "Saved attachment history is unavailable")?;
        let end = match before {
            None => versions.len(),
            Some(cursor) => versions
                .iter()
                .position(|version| version.operation().to_string() == cursor)
                .ok_or("The version cursor does not belong to this project")?,
        };
        let start = end.saturating_sub(50);
        let page = &versions[start..end];
        Ok(Json::object([
            ("schema", Json::text("mesh.attachment-versions/v1")),
            ("project", Json::text(id)),
            ("before", before.map_or(Json::Null, Json::text)),
            (
                "versions",
                Json::Array(
                    page.iter()
                        .rev()
                        .map(|version| Json::text(version.operation().to_string()))
                        .collect(),
                ),
            ),
            (
                "next_before",
                if start > 0 {
                    Json::text(versions[start].operation().to_string())
                } else {
                    Json::Null
                },
            ),
        ])
        .encode())
    }

    pub fn inspect(
        &self,
        id: &str,
        operation: &str,
        path: Option<&str>,
        after: Option<&str>,
    ) -> Result<String, String> {
        if path.is_some() && after.is_some() {
            return Err("Choose either file preview or entry paging".into());
        }
        let history = self
            .state
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .projects
            .get(id)
            .ok_or("This attachment is not open in this desktop session")?
            .history
            .clone()
            .ok_or("The original project or its saved history is unavailable")?;
        let inspection = match path {
            Some(path) => history.inspect_text(operation, path),
            None => history.inspect_entries(operation, after),
        }
        .map_err(|_| "The exact saved version could not be inspected")?;
        Ok(Json::object([("project", Json::text(id)), ("inspection", inspection)]).encode())
    }

    pub fn compare(
        &self,
        id: &str,
        base: &str,
        target: &str,
        after: Option<&str>,
    ) -> Result<String, String> {
        let history = self
            .state
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .projects
            .get(id)
            .ok_or("This attachment is not open in this desktop session")?
            .history
            .clone()
            .ok_or("The original project or its saved history is unavailable")?;
        let comparison = history
            .compare_versions(base, target, after)
            .map_err(|_| "The exact saved versions could not be compared")?;
        Ok(Json::object([("project", Json::text(id)), ("comparison", comparison)]).encode())
    }

    pub(crate) fn review_history(&self, id: &str) -> Result<ProvisionedAttachment, String> {
        self.state
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .projects
            .get(id)
            .ok_or("This attachment is not open in this desktop session")?
            .history
            .clone()
            .ok_or("Saved history is unavailable".into())
    }

    pub(crate) fn file_change_history(
        &self,
        id: &str,
    ) -> Result<(ProvisionedAttachment, u64), String> {
        let state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        let project = state.projects.get(id).ok_or("This project is not open")?;
        if project.detached {
            return Err("Reattach the project before changing a working file".into());
        }
        Ok((
            project
                .history
                .clone()
                .ok_or("Saved history is unavailable")?,
            project.generation,
        ))
    }

    pub(crate) fn confirm_file_change<T>(
        &self,
        id: &str,
        generation: u64,
        apply: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        // Serialize session controls with the confirmed mutation. Detach or reattach during
        // the dialog invalidates the proposal even though its native directory remains pinned.
        let state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        let project = state.projects.get(id).ok_or("This project is not open")?;
        if project.detached || project.generation != generation {
            return Err("The project session changed during confirmation. Inspect recovery before retrying.".into());
        }
        apply()
    }

    pub fn file_recovery(
        &self,
        id: &str,
        transaction: Option<&str>,
        trusted: &mesh_daemon::TrustedReviewers,
    ) -> Result<String, String> {
        let history = self.review_history(id)?;
        let root = history
            .file_recovery_root(false)
            .map_err(|_| "File recovery storage is unavailable")?;
        let recovery = match root {
            Some(root) => history
                .inspect_integration_recovery(
                    &root,
                    transaction,
                    trusted,
                    mesh_daemon::project_attachment::ObservationLimits::default(),
                )
                .map_err(|_| "Retained files could not be inspected")?,
            None if transaction.is_some() => {
                return Err("This recovery transaction is unavailable".into())
            }
            None => Json::Null,
        };
        Ok(Json::object([
            ("schema", Json::text("mesh.desktop-attachment-recovery/v1")),
            ("project", Json::text(id)),
            ("recovery", recovery),
        ])
        .encode())
    }

    pub fn request_review(
        &self,
        id: &str,
        target: &str,
        trusted: &mesh_daemon::TrustedReviewers,
    ) -> Result<String, String> {
        let history = self.review_history(id)?;
        let actor = NativeCaptureSigner::generate()?.public_key();
        let review = history
            .request_review_with_trusted_reviewers(target, actor, trusted)
            .map_err(|_| "The exact saved review could not be recorded")?;
        Ok(Json::object([
            ("schema", Json::text("mesh.desktop-attachment-review/v1")),
            ("project", Json::text(id)),
            ("review", review),
        ])
        .encode())
    }

    pub fn reviews(
        &self,
        id: &str,
        trusted: &mesh_daemon::TrustedReviewers,
    ) -> Result<String, String> {
        let queue = self
            .review_history(id)?
            .reviews_with_trusted_reviewers(trusted)
            .map_err(|_| "Saved reviews are unavailable")?;
        Ok(Json::object([
            ("schema", Json::text("mesh.desktop-attachment-reviews/v1")),
            ("project", Json::text(id)),
            ("queue", queue),
        ])
        .encode())
    }

    pub fn review(
        &self,
        id: &str,
        bundle: &str,
        target: &str,
        trusted: &mesh_daemon::TrustedReviewers,
    ) -> Result<String, String> {
        let review = self
            .review_history(id)?
            .review_with_trusted_reviewers(bundle, target, trusted)
            .map_err(|_| "The exact saved review is unavailable")?;
        Ok(Json::object([
            ("schema", Json::text("mesh.desktop-attachment-review/v1")),
            ("project", Json::text(id)),
            ("review", review),
        ])
        .encode())
    }

    pub fn load_pins(&self) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        self.initialize(&mut state, false)?;
        let pins = match &state.storage {
            Some(storage) => storage
                .load_comparison_pins()
                .map_err(|_| "Saved pin selectors need reconciliation")?,
            None => AttachmentPinState {
                revision: 0,
                pins: Vec::new(),
            },
        };
        Ok(pins.to_json().encode())
    }

    pub fn save_pins(&self, snapshot: &str) -> Result<String, String> {
        let snapshot =
            AttachmentPinState::parse_projection(snapshot).map_err(|_| "Invalid pin selectors")?;
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        self.initialize(&mut state, true)?;
        let stored = state
            .storage
            .as_ref()
            .ok_or(UNAVAILABLE)?
            .save_comparison_pins(snapshot.revision, snapshot.pins)
            .map_err(|_| "Pin state changed or could not be saved")?;
        Ok(stored.to_json().encode())
    }

    pub fn load_fleet_pins(&self) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        self.initialize(&mut state, false)?;
        let pins = match &state.storage {
            Some(storage) => storage
                .load_fleet_pins()
                .map_err(|_| "Saved pin selectors need reconciliation")?,
            None => FleetPinState {
                revision: 0,
                pins: Vec::new(),
            },
        };
        Ok(pins.to_json().encode())
    }

    pub fn save_fleet_pins(&self, snapshot: &str) -> Result<String, String> {
        let snapshot =
            FleetPinState::parse_projection(snapshot).map_err(|_| "Invalid pin selectors")?;
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        self.initialize(&mut state, true)?;
        let stored = state
            .storage
            .as_ref()
            .ok_or(UNAVAILABLE)?
            .save_fleet_pins(snapshot.revision, snapshot.pins)
            .map_err(|_| "Pin state changed or could not be saved")?;
        Ok(stored.to_json().encode())
    }

    pub fn comparison_path(
        &self,
        id: &str,
        base: &str,
        target: &str,
        path: &str,
    ) -> Result<String, String> {
        let history = self
            .state
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .projects
            .get(id)
            .ok_or("This attachment is not open in this desktop session")?
            .history
            .clone()
            .ok_or("Saved history is unavailable")?;
        let comparison = history
            .comparison_path(base, target, path)
            .map_err(|_| "Saved comparison path is unavailable")?;
        Ok(Json::object([("project", Json::text(id)), ("comparison", comparison)]).encode())
    }

    pub fn control(&self, id: &str, generation: &str, action: &str) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        let project = state
            .projects
            .get(id)
            .ok_or("This attachment is not open in this desktop session")?;
        if project.generation.to_string() != generation {
            return Err("This attachment control belongs to an older capture session".into());
        }
        if project.detached && !matches!(action, "detach" | "reattach") {
            return Err("Reattach this project before controlling capture".into());
        }
        match action {
            "detach" => {
                let next = state.generation.checked_add(1).ok_or(UNAVAILABLE)?;
                let project = state.projects.get_mut(id).ok_or(UNAVAILABLE)?;
                if let Some(service) = project.service.as_mut() {
                    project.recovered = service
                        .stop_capture_and_join()
                        .map_err(|_| "Capture stop needs reconciliation")?;
                }
                state
                    .storage
                    .as_ref()
                    .ok_or(UNAVAILABLE)?
                    .set_detached(id, true)
                    .map_err(|_| "Capture stopped, but detachment could not be confirmed")?;
                let project = state.projects.get_mut(id).ok_or(UNAVAILABLE)?;
                project.detached = true;
                project.generation = next;
                state.generation = next;
            }
            "reattach" => {
                if !project.detached {
                    return Err("This project is already attached".into());
                }
                let next = state.generation.checked_add(1).ok_or(UNAVAILABLE)?;
                let storage = state.storage.as_ref().ok_or(UNAVAILABLE)?;
                let history = storage
                    .reopen(id)
                    .map_err(|_| "The original project must be available before reattachment")?;
                storage
                    .set_detached(id, false)
                    .map_err(|_| "Reattachment could not be confirmed")?;
                let project = state.projects.get_mut(id).ok_or(UNAVAILABLE)?;
                project.detached = false;
                project.history = Some(history);
                project.generation = next;
                project.recovery = Some("restored-stopped");
                state.generation = next;
            }
            "capture" => {
                if !project
                    .service
                    .as_ref()
                    .is_some_and(AttachmentCaptureService::request_capture)
                {
                    return Err("Resume capture before requesting a saved version".into());
                }
            }
            "stop" => {
                if let Some(service) = &project.service {
                    service.request_stop();
                }
            }
            "resume" => {
                if !matches!(
                    project.status().phase,
                    CapturePhase::Stopped | CapturePhase::Failed
                ) {
                    return Err("Wait for capture to stop before resuming".into());
                }
                let source = project.source.clone();
                let lane = project.lane.clone();
                let history = match &project.history {
                    Some(history) => history.clone(),
                    None => state
                        .storage
                        .as_ref()
                        .ok_or(UNAVAILABLE)?
                        .reopen(id)
                        .map_err(|_| UNAVAILABLE)?,
                };
                let next = state.generation.checked_add(1).ok_or(UNAVAILABLE)?;
                let service = history
                    .start_capture(NativeCaptureSigner::generate()?, CaptureSchedule::default())
                    .map_err(|_| UNAVAILABLE)?;
                state.generation = next;
                state.projects.insert(
                    id.to_owned(),
                    Project {
                        lane,
                        detached: false,
                        source,
                        generation: next,
                        service: Some(service),
                        history: Some(history),
                        recovered: stopped_status(),
                        recovery: None,
                    },
                );
            }
            _ => return Err("Unknown attachment control".into()),
        }
        Ok(state
            .projects
            .get(id)
            .ok_or(UNAVAILABLE)?
            .projection(id)
            .encode())
    }
}

fn unavailable_lane() -> Json {
    Json::object([("schema", Json::text("mesh.attachment-lane-unavailable/v1"))])
}

fn lane_origin(storage: &AttachmentStorage, history: &ProvisionedAttachment) -> Json {
    storage.lane_origin(history).map_or_else(
        |_| unavailable_lane(),
        |origin| origin.unwrap_or(Json::Null),
    )
}

fn stopped_status() -> CaptureStatus {
    CaptureStatus {
        native_signal_state: NativeSignalState::Stopped,
        native_events: false,
        event_signals: 0,
        revision: 0,
        phase: CapturePhase::Stopped,
        last_outcome: CaptureOutcome::Pending,
        saved_version: None,
        attempts: 0,
        versions_saved: 0,
        last_attempt_duration: None,
        last_attempt_age: None,
        last_complete_capture_age: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    #[test]
    fn fleet_pin_storage_reopens_without_creating_or_adopting_work() {
        use mesh_daemon::project_attachment::FleetPin;
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-fleet-pins-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let host = AttachmentHost::new(&root);
        assert_eq!(
            FleetPinState::parse_projection(&host.load_fleet_pins().unwrap())
                .unwrap()
                .revision,
            0
        );
        assert!(!root.join("attached-projects").exists());
        assert!(host.save_fleet_pins("{}").is_err());
        assert!(!root.join("attached-projects").exists());
        let snapshot = FleetPinState {
            revision: 0,
            pins: vec![FleetPin {
                key: "1".into(),
                objective: format!("fleet-{}", "a".repeat(64)),
                lane: "worker".into(),
                checkpoint: "checkpoint".into(),
                version: "b".repeat(64),
                bundle: "c".repeat(64),
                source_version: "d".repeat(64),
                input_after: None,
                input_object: None,
                input_open: false,
                input_layout: "inline".into(),
                review_object: None,
                review_mode: "content".into(),
                review_layout: "split".into(),
            }],
        };
        let saved = host.save_fleet_pins(&snapshot.to_json().encode()).unwrap();
        assert!(host.state.lock().unwrap().projects.is_empty());
        assert!(!root.join("fleets").exists());
        drop(host);
        let reopened = AttachmentHost::new(&root);
        assert_eq!(reopened.load_fleet_pins().unwrap(), saved);
        assert!(reopened.state.lock().unwrap().projects.is_empty());
        assert!(reopened
            .save_fleet_pins(&snapshot.to_json().encode())
            .is_err());
        #[cfg(target_os = "macos")]
        assert!(reopened.current_fleet(&snapshot.pins[0].objective).is_err());
        assert!(!root.join("fleets").exists());
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_lanes_capture_independently_recover_ancestry_and_preserve_the_source_session() {
        fn wait_saved(host: &AttachmentHost, id: &str, previous: Option<String>) -> String {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let state = host.state.lock().unwrap();
                let service = state.projects.get(id).unwrap().service.as_ref().unwrap();
                let status = service.status();
                if let Some(version) = status
                    .saved_version
                    .map(|value| value.operation().to_string())
                {
                    if Some(&version) != previous.as_ref() {
                        return version;
                    }
                }
                assert!(Instant::now() < deadline);
                service.wait_for_update(status.revision, Duration::from_millis(50));
            }
        }
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-manual-lanes-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("work"), "saved source").unwrap();
        let host = AttachmentHost::new(&root);
        let attached = Json::parse(&host.attach(&source).unwrap()).unwrap();
        let id = attached.get("id").unwrap().as_text().unwrap();
        let generation = attached.get("generation").unwrap().as_text().unwrap();
        let version = wait_saved(&host, id, None);
        fs::write(source.join("work"), "source continues").unwrap();
        let result = host
            .open_version_lane(id, &version, &"a".repeat(32))
            .unwrap();
        let result = Json::parse(&result).unwrap();
        let child = result.get("project").unwrap().as_text().unwrap();
        let initial = wait_saved(&host, child, None);
        let child_history = host.review_history(child).unwrap();
        assert_eq!(
            fs::read(child_history.project().root().join("work")).unwrap(),
            b"saved source"
        );
        fs::write(
            child_history.project().root().join("work"),
            "independent work",
        )
        .unwrap();
        {
            let state = host.state.lock().unwrap();
            state.projects[child]
                .service
                .as_ref()
                .unwrap()
                .request_capture();
        }
        wait_saved(&host, child, Some(initial));
        assert_eq!(child_history.saved_versions().unwrap().len(), 2);
        let repeated = Json::parse(
            &host
                .open_version_lane(id, &version, &"a".repeat(32))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(repeated.get("project"), Some(&Json::text(child)));
        assert_eq!(
            fs::read(child_history.project().root().join("work")).unwrap(),
            b"independent work"
        );
        assert_eq!(fs::read(source.join("work")).unwrap(), b"source continues");
        {
            let mut state = host.state.lock().unwrap();
            assert_eq!(state.projects[id].generation.to_string(), generation);
            assert_eq!(
                state.projects[child].lane.get("source_version"),
                Some(&Json::text(&version))
            );
            for (_, mut project) in std::mem::take(&mut state.projects) {
                if let Some(service) = project.service.take() {
                    service.stop_and_join().unwrap();
                }
            }
        }
        drop(host);
        let restarted = AttachmentHost::new(&root);
        restarted.projects().unwrap();
        {
            let state = restarted.state.lock().unwrap();
            assert_eq!(state.projects.len(), 2);
            assert!(state.projects[child].service.is_none());
            assert_eq!(
                state.projects[child].lane.get("source_project"),
                Some(&Json::text(id))
            );
        }
        drop(restarted);
        fs::rename(child_history.project().root(), root.join("offline-lane")).unwrap();
        let offline = AttachmentHost::new(&root);
        offline.projects().unwrap();
        assert_eq!(
            offline.state.lock().unwrap().projects[child].lane,
            unavailable_lane()
        );
        assert_eq!(
            fs::read(root.join("offline-lane/work")).unwrap(),
            b"independent work"
        );
        drop(offline);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_recovery_inspection_does_not_provision_and_detach_invalidates_confirmation() {
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-file-recovery-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("work"), "ongoing work").unwrap();
        let host = AttachmentHost::new(&root);
        let attached = Json::parse(&host.attach(&source).unwrap()).unwrap();
        let id = attached.get("id").unwrap().as_text().unwrap();
        let (history, generation) = host.file_change_history(id).unwrap();
        let response = Json::parse(
            &host
                .file_recovery(id, None, &mesh_daemon::TrustedReviewers::default())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(response.get("recovery"), Some(&Json::Null));
        assert!(!history.metadata_path().join("file-recovery").exists());
        assert!(host
            .file_recovery("unknown", None, &mesh_daemon::TrustedReviewers::default())
            .is_err());
        assert_eq!(
            host.confirm_file_change(id, generation, || Ok(7)).unwrap(),
            7
        );
        let detached =
            Json::parse(&host.control(id, &generation.to_string(), "detach").unwrap()).unwrap();
        assert!(host.file_change_history(id).is_err());
        assert!(host
            .confirm_file_change(id, generation, || -> Result<(), String> {
                panic!("stale mutation invoked")
            })
            .is_err());
        host.control(
            id,
            detached.get("generation").unwrap().as_text().unwrap(),
            "reattach",
        )
        .unwrap();
        assert!(host
            .confirm_file_change(id, generation, || -> Result<(), String> {
                panic!("old confirmation revived")
            })
            .is_err());
        assert_eq!(fs::read(source.join("work")).unwrap(), b"ongoing work");
        drop(host);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn desktop_sessions_deduplicate_and_reject_stale_controls() {
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-attachments-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("work"), "ongoing work").unwrap();
        let host = AttachmentHost::new(&root);
        let first = Json::parse(&host.attach(&source).unwrap()).unwrap();
        let id = first.get("id").unwrap().as_text().unwrap();
        let generation = first.get("generation").unwrap().as_text().unwrap();
        let again = Json::parse(&host.attach(&source).unwrap()).unwrap();
        assert_eq!(again.get("id"), first.get("id"));
        assert_eq!(again.get("generation"), first.get("generation"));
        assert_eq!(
            fs::read_dir(root.join("attached-projects"))
                .unwrap()
                .count(),
            1
        );
        host.control(id, generation, "stop").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = host.state.lock().unwrap();
            let service = state.projects.get(id).unwrap().service.as_ref().unwrap();
            let status = service.status();
            if status.phase == CapturePhase::Stopped {
                break;
            }
            assert!(Instant::now() < deadline);
            service.wait_for_update(status.revision, Duration::from_millis(50));
        }
        let stopped = Json::parse(&host.attach(&source).unwrap()).unwrap();
        assert_eq!(stopped.get("generation"), first.get("generation"));
        let resumed = Json::parse(&host.control(id, generation, "resume").unwrap()).unwrap();
        assert_ne!(resumed.get("generation"), first.get("generation"));
        assert!(host.control(id, generation, "stop").is_err());
        assert_eq!(fs::read(source.join("work")).unwrap(), b"ongoing work");
        let projects = Json::parse(&host.projects().unwrap()).unwrap();
        assert_eq!(
            projects.get("schema").and_then(Json::as_text),
            Some("mesh.desktop-attachments/v1")
        );
        let mut state = host.state.lock().unwrap();
        for (_, project) in std::mem::take(&mut state.projects) {
            project.service.unwrap().stop_and_join().unwrap();
        }
        drop(state);
        drop(host);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn concurrent_attachment_requests_share_one_worker_and_replaced_source_cannot_resume() {
        use std::sync::Arc;
        let root = std::env::temp_dir().join(format!(
            "mesh-desktop-attach-concurrent-{}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("work"), "original").unwrap();
        let host = Arc::new(AttachmentHost::new(&root));
        let requests: Vec<_> = (0..8)
            .map(|_| {
                let host = host.clone();
                let source = source.clone();
                std::thread::spawn(move || host.attach(&source).unwrap())
            })
            .collect();
        let responses: Vec<_> = requests
            .into_iter()
            .map(|request| Json::parse(&request.join().unwrap()).unwrap())
            .collect();
        assert!(responses
            .iter()
            .all(|response| response.get("generation") == responses[0].get("generation")));
        let id = responses[0].get("id").unwrap().as_text().unwrap();
        let generation = responses[0].get("generation").unwrap().as_text().unwrap();
        assert_eq!(host.state.lock().unwrap().projects.len(), 1);
        host.control(id, generation, "stop").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = host.state.lock().unwrap();
            let service = state.projects[id].service.as_ref().unwrap();
            let status = service.status();
            if status.phase == CapturePhase::Stopped {
                break;
            }
            assert!(Instant::now() < deadline);
            service.wait_for_update(status.revision, Duration::from_millis(50));
        }
        fs::rename(&source, root.join("original-source")).unwrap();
        fs::create_dir(&source).unwrap();
        assert!(host.control(id, generation, "resume").is_err());
        assert!(host.control("../source", generation, "capture").is_err());
        assert!(fs::read_dir(&source).unwrap().next().is_none());
        drop(host);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linked_host_storage_is_refused_without_writing_its_target() {
        use std::os::unix::fs::symlink;
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-attach-link-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        symlink(&source, root.join("attached-projects")).unwrap();
        let host = AttachmentHost::new(&root);
        assert!(host.attach(&source).is_err());
        assert!(fs::read_dir(&source).unwrap().next().is_none());
        drop(host);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn version_pages_are_exact_and_preserve_cursor_while_new_work_is_saved() {
        use mesh_daemon::project_attachment::ObservationLimits;
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-history-pages-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("work"), "initial").unwrap();
        let host = AttachmentHost::new(&root);
        let response = Json::parse(&host.attach(&source).unwrap()).unwrap();
        let id = response.get("id").unwrap().as_text().unwrap();
        let generation = response.get("generation").unwrap().as_text().unwrap();
        host.control(id, generation, "stop").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let history = loop {
            let state = host.state.lock().unwrap();
            let project = &state.projects[id];
            let status = project.status();
            if status.phase == CapturePhase::Stopped {
                break project.history.clone().unwrap();
            }
            assert!(Instant::now() < deadline);
            project
                .service
                .as_ref()
                .unwrap()
                .wait_for_update(status.revision, Duration::from_millis(50));
        };
        let signer = NativeCaptureSigner::generate().unwrap();
        let save = |text: &str| {
            fs::write(source.join("work"), text).unwrap();
            let input = history
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            history
                .project()
                .save_capture(
                    history.metadata_path(),
                    &input,
                    signer.public_key(),
                    |payload| signer.sign(payload),
                )
                .unwrap()
        };
        for number in 0..52 {
            save(&format!("version {number}"));
        }
        let page = Json::parse(&host.versions(id, None).unwrap()).unwrap();
        let cursor = page.get("next_before").unwrap().as_text().unwrap();
        let older = host.versions(id, Some(cursor)).unwrap();
        let new = save("newer live work");
        assert_eq!(older, host.versions(id, Some(cursor)).unwrap());
        assert_ne!(
            page,
            Json::parse(&host.versions(id, None).unwrap()).unwrap()
        );
        assert!(!page.encode().contains(&new.operation().to_string()));
        assert!(host.versions(id, Some(&"f".repeat(64))).is_err());
        fs::rename(history.metadata_path(), root.join("old-history")).unwrap();
        fs::create_dir(history.metadata_path()).unwrap();
        assert!(host.versions(id, None).is_err());
        drop(host);
        fs::remove_dir_all(root).unwrap();
    }
    fn wait_for_project(
        host: &AttachmentHost,
        id: &str,
        predicate: impl Fn(&CaptureStatus) -> bool,
    ) -> CaptureStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = host.state.lock().unwrap();
            let project = &state.projects[id];
            let status = project.status();
            if predicate(&status) {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "capture did not settle: {status:?}"
            );
            project
                .service
                .as_ref()
                .unwrap()
                .wait_for_update(status.revision, Duration::from_millis(50));
        }
    }

    #[test]
    fn restart_restores_saved_projects_stopped_and_resume_catches_up_without_reselection() {
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-recovery-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("work"), "before restart").unwrap();
        let host = AttachmentHost::new(&root);
        let attached = Json::parse(&host.attach(&source).unwrap()).unwrap();
        let id = attached.get("id").unwrap().as_text().unwrap();
        let generation = attached.get("generation").unwrap().as_text().unwrap();
        let before = wait_for_project(&host, id, |status| status.saved_version.is_some())
            .saved_version
            .unwrap();
        host.control(id, generation, "stop").unwrap();
        wait_for_project(&host, id, |status| status.phase == CapturePhase::Stopped);
        drop(host);
        fs::write(source.join("work"), "edited while Mesh was closed").unwrap();
        let restarted = AttachmentHost::new(&root);
        restarted.projects().unwrap();
        let generation = {
            let state = restarted.state.lock().unwrap();
            let recovered = &state.projects[id];
            assert!(recovered.service.is_none());
            assert_eq!(recovered.status().phase, CapturePhase::Stopped);
            assert_eq!(recovered.status().saved_version, Some(before));
            assert_eq!(recovered.recovery, Some("restored-stopped"));
            recovered.generation.to_string()
        };
        assert!(restarted.control(id, &generation, "capture").is_err());
        restarted.control(id, &generation, "resume").unwrap();
        let after = wait_for_project(&restarted, id, |status| {
            status
                .saved_version
                .is_some_and(|version| version != before)
        });
        assert_eq!(after.last_outcome, CaptureOutcome::Saved);
        let generation = restarted.state.lock().unwrap().projects[id]
            .generation
            .to_string();
        restarted.control(id, &generation, "stop").unwrap();
        wait_for_project(&restarted, id, |status| {
            status.phase == CapturePhase::Stopped
        });
        drop(restarted);
        fs::rename(&source, root.join("offline")).unwrap();
        let offline = AttachmentHost::new(&root);
        offline.projects().unwrap();
        let generation = {
            let state = offline.state.lock().unwrap();
            let recovered = &state.projects[id];
            assert_eq!(recovered.recovery, Some("unavailable"));
            assert!(recovered.history.is_none());
            assert!(recovered.service.is_none());
            recovered.generation.to_string()
        };
        assert!(offline.control(id, &generation, "resume").is_err());
        fs::create_dir(&source).unwrap();
        assert!(offline.control(id, &generation, "resume").is_err());
        fs::remove_dir(&source).unwrap();
        fs::rename(root.join("offline"), &source).unwrap();
        offline.control(id, &generation, "resume").unwrap();
        wait_for_project(&offline, id, |status| status.saved_version.is_some());
        let generation = offline.state.lock().unwrap().projects[id]
            .generation
            .to_string();
        offline.control(id, &generation, "stop").unwrap();
        wait_for_project(&offline, id, |status| status.phase == CapturePhase::Stopped);
        drop(offline);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn detach_joins_capture_survives_restart_and_reattach_stays_stopped() {
        let root = std::env::temp_dir().join(format!("mesh-desktop-detach-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("work"), "before detachment").unwrap();
        let host = AttachmentHost::new(&root);
        let attached = Json::parse(&host.attach(&source).unwrap()).unwrap();
        let id = attached.get("id").unwrap().as_text().unwrap();
        let generation = attached.get("generation").unwrap().as_text().unwrap();
        let saved = wait_for_project(&host, id, |status| status.saved_version.is_some())
            .saved_version
            .unwrap();
        let detached = Json::parse(&host.control(id, generation, "detach").unwrap()).unwrap();
        assert_eq!(detached.get("detached"), Some(&Json::Bool(true)));
        let next = detached.get("generation").unwrap().as_text().unwrap();
        assert!(host.control(id, generation, "resume").is_err());
        assert!(host.control(id, next, "resume").is_err());
        assert!(host.control(id, next, "capture").is_err());
        {
            let state = host.state.lock().unwrap();
            let service = state.projects[id].service.as_ref().unwrap();
            assert_eq!(service.status().phase, CapturePhase::Stopped);
            assert!(!service.request_capture());
        }
        fs::write(source.join("work"), "normal tools continue").unwrap();
        assert!(host
            .versions(id, None)
            .unwrap()
            .contains(&saved.operation().to_string()));
        drop(host);
        let host = AttachmentHost::new(&root);
        host.projects().unwrap();
        let generation = host.state.lock().unwrap().projects[id]
            .generation
            .to_string();
        assert!(host.state.lock().unwrap().projects[id].detached);
        fs::rename(&source, root.join("offline")).unwrap();
        assert!(host.control(id, &generation, "reattach").is_err());
        fs::create_dir(&source).unwrap();
        assert!(host.control(id, &generation, "reattach").is_err());
        fs::remove_dir(&source).unwrap();
        fs::rename(root.join("offline"), &source).unwrap();
        let reattached = Json::parse(&host.control(id, &generation, "reattach").unwrap()).unwrap();
        assert_eq!(reattached.get("detached"), Some(&Json::Bool(false)));
        assert!(host.state.lock().unwrap().projects[id].service.is_none());
        let next = reattached.get("generation").unwrap().as_text().unwrap();
        host.control(id, next, "resume").unwrap();
        wait_for_project(&host, id, |status| {
            status.saved_version.is_some_and(|version| version != saved)
        });
        let next = host.state.lock().unwrap().projects[id]
            .generation
            .to_string();
        host.control(id, &next, "detach").unwrap();
        assert_eq!(
            fs::read_to_string(source.join("work")).unwrap(),
            "normal tools continue"
        );
        drop(host);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn fleet_provisioning_preserves_capture_and_reopens_only_as_unattached() {
        let root =
            std::env::temp_dir().join(format!("mesh-desktop-fleet-catalog-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("work"), "saved input").unwrap();
        let host = AttachmentHost::new(&root);
        assert!(Json::parse(&host.fleets().unwrap())
            .unwrap()
            .get("fleets")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty());
        assert!(
            !root.join("fleets").exists(),
            "viewing an empty catalogue must not provision storage"
        );
        let attached = Json::parse(&host.attach(&source).unwrap()).unwrap();
        let id = attached.get("id").unwrap().as_text().unwrap();
        let generation = attached.get("generation").unwrap().as_text().unwrap();
        let saved = wait_for_project(&host, id, |status| status.saved_version.is_some())
            .saved_version
            .unwrap()
            .operation()
            .to_string();
        let limits = r#"{"lanes":4,"concurrency":2,"depth":1,"retries":1}"#;
        let request = "e".repeat(32);
        assert!(host
            .provision_fleet(
                id,
                &request,
                "Coordinate",
                &saved,
                r#"{"lanes":4,"concurrency":2,"depth":1,"retries":1,"path":"/elsewhere"}"#
            )
            .is_err());
        assert!(!root.join("fleets").exists());
        let allocated = host
            .provision_fleet(id, &request, "Coordinate", &saved, limits)
            .unwrap();
        assert_eq!(
            Json::parse(&allocated).unwrap().get("started"),
            Some(&Json::Bool(false))
        );
        assert_eq!(
            host.provision_fleet(id, &request, "Coordinate", &saved, limits)
                .unwrap(),
            allocated
        );
        assert_eq!(
            host.state.lock().unwrap().projects[id]
                .generation
                .to_string(),
            generation
        );
        fs::write(source.join("work"), "original continues").unwrap();
        wait_for_project(&host, id, |status| {
            status
                .saved_version
                .is_some_and(|version| version.operation().to_string() != saved)
        });
        let before = Json::parse(&host.fleets().unwrap()).unwrap();
        let row = &before.get("fleets").unwrap().as_array().unwrap()[0];
        assert_eq!(row.get("ownership"), Some(&Json::text("current-host")));
        assert!(row
            .get("state")
            .unwrap()
            .get("lanes")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .all(|lane| lane.get("run") == Some(&Json::Null)));
        let objective = row.get("objective").unwrap().as_text().unwrap();
        assert!(host.current_fleet(objective).is_ok());
        drop(host);
        let reopened = AttachmentHost::new(&root);
        assert!(reopened.current_fleet(objective).is_err());
        let after = Json::parse(&reopened.fleets().unwrap()).unwrap();
        assert!(reopened.current_fleet(objective).is_err());
        let recovered = &after.get("fleets").unwrap().as_array().unwrap()[0];
        assert_eq!(recovered.get("state"), row.get("state"));
        assert_eq!(
            recovered.get("ownership"),
            Some(&Json::text("restored-unattached"))
        );
        assert_eq!(
            fs::read(source.join("work")).unwrap(),
            b"original continues"
        );
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }
}
