//! Application-owned local scheduling. Native services and executables never come from the renderer.
//! Errors suspend dispatch; polling/cancellation retain the same owned handles. No automatic adoption.

use crate::attachment_capture::NativeCaptureSigner;
use mesh_daemon::fleet::host::{CodexFleetHost, WorkerObservation, WorkerSignerFactory};
use mesh_daemon::fleet::provider::CodexAdapter;
use mesh_daemon::fleet::service::{CheckpointSigner, FleetService};
use mesh_daemon::fleet::Command;
use mesh_daemon::ipc::{Json, Unavailable};
use mesh_daemon::LiveDaemon;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const UNAVAILABLE: &str = "Fleet execution is unavailable or needs recovery";
const INTERVAL: Duration = Duration::from_millis(250);

struct NativeWorkerSigners;
impl WorkerSignerFactory for NativeWorkerSigners {
    fn signer(&self, _lane: &str, _run: &str) -> Result<Arc<dyn CheckpointSigner>, Unavailable> {
        NativeCaptureSigner::generate().map_err(|_| {
            Unavailable::new(
                "fleet-native-signer",
                "The native worker identity is unavailable.",
            )
        })
    }
}

#[derive(Default)]
struct Progress {
    observations: Vec<WorkerObservation>,
    observed_at: Option<SystemTime>,
    attention: bool,
    stopping: bool,
}
struct Running {
    service: Arc<FleetService>,
    progress: Arc<Mutex<Progress>>,
    wake: mpsc::SyncSender<()>,
    thread: JoinHandle<()>,
}

/// One scheduling loop per explicitly started objective, bounded by the native catalogue limit.
/// Dropping this owner disconnects the loops and requests cancellation. It never proves process-tree
/// termination. OS exit/crash still needs durable reconciliation and cannot authorize a relaunch.
#[derive(Default)]
pub(crate) struct FleetHosts(Mutex<BTreeMap<String, Running>>);
impl FleetHosts {
    pub(crate) fn start(
        &self,
        service: Arc<FleetService>,
        daemon: &LiveDaemon,
        adapter: CodexAdapter,
        endpoint: PathBuf,
    ) -> Result<(), String> {
        let objective = service.objective().map_err(|_| UNAVAILABLE)?;
        let mut held = self.0.lock().map_err(|_| UNAVAILABLE)?;
        if let Some(existing) = held.get(&objective) {
            return if Arc::ptr_eq(&existing.service, &service) && !existing.thread.is_finished() {
                Ok(())
            } else {
                Err(UNAVAILABLE.into())
            };
        }
        let state = service.native_state().map_err(|_| UNAVAILABLE)?;
        if held.len() >= 16
            || state.cancelled
            || state.lanes.is_empty()
            || state.lanes.values().any(|lane| !lane.runs.is_empty())
        {
            return Err(UNAVAILABLE.into());
        }
        let host = CodexFleetHost::new(
            service.clone(),
            adapter,
            endpoint,
            Arc::new(NativeWorkerSigners),
        )
        .map_err(|_| UNAVAILABLE)?;
        // Exact-instance registration is retryable if thread creation fails before any launch.
        // A different service can never replace an existing route for this objective.
        daemon
            .register_fleet(service.clone())
            .map_err(|_| UNAVAILABLE)?;
        let progress = Arc::new(Mutex::new(Progress::default()));
        let (wake, receiver) = mpsc::sync_channel(1);
        let observed = progress.clone();
        let owned = service.clone();
        let thread = thread::Builder::new()
            .name("mesh-fleet".into())
            .spawn(move || run(host, owned, observed, receiver))
            .map_err(|_| UNAVAILABLE)?;
        held.insert(
            objective,
            Running {
                service,
                progress,
                wake,
                thread,
            },
        );
        Ok(())
    }

    pub(crate) fn stop(&self, service: &Arc<FleetService>) -> Result<(), String> {
        let objective = service.objective().map_err(|_| UNAVAILABLE)?;
        let held = self.0.lock().map_err(|_| UNAVAILABLE)?;
        let running = held.get(&objective);
        if running.is_some_and(|running| !Arc::ptr_eq(&running.service, service)) {
            return Err(UNAVAILABLE.into());
        }
        // Stable retry records the same cancellation. Cancellation also prevents a provisioned,
        // not-yet-started fleet from launching. No slot or custody is released by this request.
        service
            .native_command("desktop-stop", Command::Cancel)
            .map_err(|_| UNAVAILABLE)?;
        if let Some(running) = running {
            running.progress.lock().map_err(|_| UNAVAILABLE)?.stopping = true;
            let _ = running.wake.try_send(());
        }
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> Result<String, String> {
        let held = self.0.lock().map_err(|_| UNAVAILABLE)?;
        let mut rows = Vec::with_capacity(held.len());
        for (objective, running) in held.iter() {
            let progress = running.progress.lock().map_err(|_| UNAVAILABLE)?;
            rows.push(Json::object([
                ("objective", Json::text(objective)),
                (
                    "status",
                    Json::text(if progress.attention || running.thread.is_finished() {
                        "needs-attention"
                    } else if progress.stopping {
                        "stop-requested"
                    } else if progress.observed_at.is_some() {
                        "monitoring"
                    } else {
                        "starting"
                    }),
                ),
                ("stop_requested", Json::Bool(progress.stopping)),
                ("observed_at", timestamp(progress.observed_at)),
                (
                    "workers",
                    Json::Array(progress.observations.iter().map(observation).collect()),
                ),
            ]));
        }
        Ok(Json::object([
            ("schema", Json::text("mesh.desktop-fleet-activity/v1")),
            ("fleets", Json::Array(rows)),
        ])
        .encode())
    }
}

fn run(
    mut host: CodexFleetHost,
    service: Arc<FleetService>,
    progress: Arc<Mutex<Progress>>,
    wake: mpsc::Receiver<()>,
) {
    let mut attention = false;
    loop {
        let result = if attention {
            host.poll_owned()
        } else {
            host.tick()
        };
        let Ok(mut held) = progress.lock() else {
            let _ = service.native_command("desktop-owner-lost", Command::Cancel);
            let _ = host.poll_owned();
            return;
        };
        match result {
            Ok(observations) => {
                held.observations = observations;
                held.observed_at = Some(SystemTime::now());
            }
            Err(_) => {
                attention = true;
                held.attention = true;
            }
        }
        drop(held);
        match wake.recv_timeout(INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // Best effort on graceful owner drop. Uncertain exits are deliberately retained.
                let _ = service.native_command("desktop-owner-lost", Command::Cancel);
                let _ = host.poll_owned();
                return;
            }
        }
    }
}
fn timestamp(time: Option<SystemTime>) -> Json {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(Json::Null, |time| Json::text(time.as_millis().to_string()))
}
fn observation(value: &WorkerObservation) -> Json {
    let activity = &value.activity;
    Json::object([
        ("lane", Json::text(&value.lane)),
        ("run", Json::text(&value.run)),
        ("observed_at", timestamp(Some(value.observed_at))),
        (
            "thread",
            activity.thread.as_ref().map_or(Json::Null, Json::text),
        ),
        (
            "activity",
            activity.activity.as_ref().map_or(Json::Null, Json::text),
        ),
        ("events", Json::text(activity.events.to_string())),
        (
            "stderr_lines",
            Json::text(activity.stderr_lines.to_string()),
        ),
        ("turn_completed", Json::Bool(activity.turn_completed)),
        ("failed", Json::Bool(activity.failed)),
        ("streams_closed", Json::Bool(activity.streams_closed)),
        ("outcome", value.outcome.map_or(Json::Null, Json::Bool)),
    ])
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use mesh_daemon::fleet::catalog::{AttachedFleetRequest, NativeFleetDirectory};
    use mesh_daemon::fleet::{Limits, RunState};
    use mesh_daemon::ipc::{nothing_to_recover, Operations as _, StartupSummary};
    use mesh_daemon::project_attachment::{
        AttachmentStorage, ObservationLimits, ProvisionedAttachment,
    };
    use mesh_daemon::{CheckpointRuntimeParameters, TrustedReviewers};
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::Instant;

    struct Fixture {
        root: PathBuf,
        catalog: NativeFleetDirectory,
        history: ProvisionedAttachment,
        request: AttachedFleetRequest,
        service: Arc<FleetService>,
        daemon: LiveDaemon,
        adapter: CodexAdapter,
    }
    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "mesh-desktop-fleet-host-{name}-{}",
                std::process::id()
            ));
            fs::create_dir(&root).unwrap();
            for child in ["source", "metadata", "fleets"] {
                let path = root.join(child);
                fs::create_dir(&path).unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            fs::write(root.join("source/work"), "original").unwrap();
            let history = AttachmentStorage::open(&root.join("metadata"))
                .unwrap()
                .provision(&root.join("source"))
                .unwrap();
            let input = history
                .project()
                .capture_inputs(ObservationLimits::default())
                .unwrap();
            let signer = NativeCaptureSigner::generate().unwrap();
            let version = history
                .project()
                .save_capture(
                    history.metadata_path(),
                    &input,
                    signer.public_key(),
                    |payload| signer.sign(payload),
                )
                .unwrap()
                .operation();
            let parameters = CheckpointRuntimeParameters {
                idle_interval: Some(Duration::from_millis(10)),
                maximum_uncheckpointed_bytes: Some(65536),
                maximum_uncheckpointed_interval: Some(Duration::from_secs(60)),
            };
            let catalog = NativeFleetDirectory::open(
                &root.join("fleets"),
                TrustedReviewers::default(),
                parameters,
            )
            .unwrap();
            let request = AttachedFleetRequest {
                request: "a".repeat(32),
                goal: "Coordinate".into(),
                version,
                limits: Limits {
                    lanes: 4,
                    concurrency: 2,
                    depth: 1,
                    retries: 0,
                },
            };
            let service = catalog.create_attached(&history, &request).unwrap();
            let daemon = LiveDaemon::with_checkpoint_runtime(
                StartupSummary::from(&nothing_to_recover()),
                parameters,
            )
            .unwrap();
            let original = root.join("source");
            let preview = daemon
                .preview_folder_import(original.to_str().unwrap())
                .unwrap();
            daemon
                .confirm_folder_import(
                    original.to_str().unwrap(),
                    root.join("selected.mesh").to_str().unwrap(),
                    preview.get("summary").unwrap().as_text().unwrap(),
                )
                .unwrap();
            let executable = root.join("provider");
            fs::write(&executable, "#!/bin/sh\ncat >/dev/null\necho launch >> launches\nwhile [ ! -f release ]; do sleep 0.02; done\nprintf '%s\\n' '{\"type\":\"turn.completed\"}'\n").unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            let adapter = CodexAdapter::with_desktop_bridge(&executable, &executable).unwrap();
            Self {
                root,
                catalog,
                history,
                request,
                service,
                daemon,
                adapter,
            }
        }
        fn start(&self, hosts: &FleetHosts) -> Result<(), String> {
            hosts.start(
                self.service.clone(),
                &self.daemon,
                self.adapter.clone(),
                self.root.join("ipc.sock"),
            )
        }
        fn working(&self) -> PathBuf {
            PathBuf::from(
                self.service
                    .native_state()
                    .unwrap()
                    .lanes
                    .values()
                    .next()
                    .unwrap()
                    .workspace
                    .as_ref()
                    .unwrap()
                    .root(),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn wait(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "native fleet loop did not progress"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn status(hosts: &FleetHosts) -> Json {
        Json::parse(&hosts.snapshot().unwrap())
            .unwrap()
            .get("fleets")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .clone()
    }
    fn shutdown(hosts: FleetHosts) {
        // Test joins after signalling owner disconnect; production drop retains no termination claim.
        for (_, running) in hosts.0.into_inner().unwrap() {
            drop(running.wake);
            running.thread.join().unwrap();
        }
    }

    #[test]
    fn app_loop_starts_once_observes_and_stops_without_changing_source_or_selection() {
        let f = Fixture::new("control");
        let selected = f.daemon.workspace_state().unwrap();
        let hosts = FleetHosts::default();
        f.start(&hosts).unwrap();
        f.start(&hosts).unwrap();
        wait(|| f.working().join("launches").exists());
        wait(|| {
            !status(&hosts)
                .get("workers")
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        });
        assert_eq!(
            fs::read_to_string(f.working().join("launches")).unwrap(),
            "launch\n"
        );
        assert_eq!(
            status(&hosts).get("status"),
            Some(&Json::text("monitoring"))
        );
        assert_eq!(
            f.service
                .native_state()
                .unwrap()
                .lanes
                .values()
                .next()
                .unwrap()
                .runs
                .len(),
            1
        );
        fs::write(f.root.join("source/work"), "editor continues").unwrap();
        hosts.stop(&f.service).unwrap();
        hosts.stop(&f.service).unwrap();
        wait(|| {
            status(&hosts).get("workers").unwrap().as_array().unwrap()[0].get("outcome")
                == Some(&Json::Bool(false))
        });
        assert_eq!(
            status(&hosts).get("status"),
            Some(&Json::text("stop-requested"))
        );
        assert_eq!(
            f.service
                .native_state()
                .unwrap()
                .lanes
                .values()
                .next()
                .unwrap()
                .runs[0]
                .state,
            RunState::Stopping
        );
        assert_eq!(f.daemon.workspace_state().unwrap(), selected);
        assert_eq!(
            fs::read_to_string(f.root.join("source/work")).unwrap(),
            "editor continues"
        );
        shutdown(hosts);
    }

    #[test]
    fn launch_error_suspends_new_dispatch_and_retains_uncertain_attempt() {
        let f = Fixture::new("error");
        let hosts = FleetHosts::default();
        fs::remove_file(f.root.join("provider")).unwrap();
        f.start(&hosts).unwrap();
        wait(|| status(&hosts).get("status") == Some(&Json::text("needs-attention")));
        let before = f.service.native_state().unwrap();
        assert_eq!(before.lanes.values().next().unwrap().runs.len(), 1);
        f.service
            .create_root_from_attachment(
                "queued",
                "Do more",
                "codex",
                &f.history,
                f.request.version,
            )
            .unwrap();
        let observed = status(&hosts).get("observed_at").unwrap().clone();
        wait(|| status(&hosts).get("observed_at") != Some(&observed));
        f.start(&hosts).unwrap(); // A duplicate start cannot clear the latched fault.
        assert_eq!(
            status(&hosts).get("status"),
            Some(&Json::text("needs-attention"))
        );
        let after = f.service.native_state().unwrap();
        assert_eq!(
            after
                .lanes
                .values()
                .filter(|lane| !lane.runs.is_empty())
                .count(),
            1
        );
        assert!(f
            .catalog
            .current_service(&f.service.objective().unwrap())
            .is_ok());
        hosts.stop(&f.service).unwrap();
        shutdown(hosts);
    }

    #[test]
    fn cancellation_before_start_never_launches_and_owner_drop_records_cancel() {
        let f = Fixture::new("before-start");
        let hosts = FleetHosts::default();
        hosts.stop(&f.service).unwrap();
        assert!(f.start(&hosts).is_err());
        assert!(f
            .service
            .native_state()
            .unwrap()
            .lanes
            .values()
            .all(|lane| lane.runs.is_empty()));
        let mut request = f.request.clone();
        request.request = "b".repeat(32);
        let second = f.catalog.create_attached(&f.history, &request).unwrap();
        hosts
            .start(
                second.clone(),
                &f.daemon,
                f.adapter.clone(),
                f.root.join("ipc.sock"),
            )
            .unwrap();
        shutdown(hosts);
        assert!(second.native_state().unwrap().cancelled);
        assert_eq!(
            fs::read_to_string(f.root.join("source/work")).unwrap(),
            "original"
        );
    }
}
