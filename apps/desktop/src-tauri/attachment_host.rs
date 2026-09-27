//! Native desktop attachment sessions. The renderer supplies a source selection, never store paths.

use crate::attachment_capture::NativeCaptureSigner;
use mesh_daemon::ipc::Json;
use mesh_daemon::project_attachment::{
    AttachmentCaptureService, AttachmentStorage, CapturePhase, CaptureSchedule,
    ProvisionedAttachment,
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
    state: Mutex<HostState>,
}
#[derive(Default)]
struct HostState {
    storage: Option<AttachmentStorage>,
    projects: BTreeMap<String, Project>,
    generation: u64,
}
struct Project {
    source: PathBuf,
    generation: u64,
    service: AttachmentCaptureService,
    history: ProvisionedAttachment,
}
impl Project {
    fn projection(&self, id: &str) -> Json {
        Json::object([
            ("id", Json::text(id)),
            ("generation", Json::text(self.generation.to_string())),
            ("root", Json::text(self.source.to_string_lossy())),
            ("capture", self.service.status().to_json()),
        ])
    }
}
impl AttachmentHost {
    pub fn new(application_data: &Path) -> Self {
        Self {
            storage_path: application_data.join("attached-projects"),
            state: Mutex::new(HostState::default()),
        }
    }

    pub fn attach(&self, source: &Path) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        if state.storage.is_none() {
            match DirBuilder::new().mode(0o700).create(&self.storage_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(UNAVAILABLE.into()),
            }
            state.storage =
                Some(AttachmentStorage::open(&self.storage_path).map_err(|_| UNAVAILABLE)?);
        }
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
            source,
            generation,
            service,
            history: provisioned,
        };
        let response = project.projection(&id).encode();
        state.projects.insert(id, project);
        Ok(response)
    }

    pub fn projects(&self) -> Result<String, String> {
        let state = self.state.lock().map_err(|_| UNAVAILABLE)?;
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
            .clone();
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

    pub fn control(&self, id: &str, generation: &str, action: &str) -> Result<String, String> {
        let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
        let project = state
            .projects
            .get(id)
            .ok_or("This attachment is not open in this desktop session")?;
        if project.generation.to_string() != generation {
            return Err("This attachment control belongs to an older capture session".into());
        }
        match action {
            "capture" => {
                if !project.service.request_capture() {
                    return Err("Resume capture before requesting a saved version".into());
                }
            }
            "stop" => project.service.request_stop(),
            "resume" => {
                if !matches!(
                    project.service.status().phase,
                    CapturePhase::Stopped | CapturePhase::Failed
                ) {
                    return Err("Wait for capture to stop before resuming".into());
                }
                let source = project.source.clone();
                let history = project.history.clone();
                let next = state.generation.checked_add(1).ok_or(UNAVAILABLE)?;
                let service = history
                    .start_capture(NativeCaptureSigner::generate()?, CaptureSchedule::default())
                    .map_err(|_| UNAVAILABLE)?;
                state.generation = next;
                state.projects.insert(
                    id.to_owned(),
                    Project {
                        source,
                        generation: next,
                        service,
                        history,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

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
            let service = &state.projects.get(id).unwrap().service;
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
            project.service.stop_and_join().unwrap();
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
            let service = &state.projects[id].service;
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
            let status = project.service.status();
            if status.phase == CapturePhase::Stopped {
                break project.history.clone();
            }
            assert!(Instant::now() < deadline);
            project
                .service
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
}
