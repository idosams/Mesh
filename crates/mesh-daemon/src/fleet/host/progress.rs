use super::super::service::WorkerProgressSave;
use super::*;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
const INTERVAL: Duration = Duration::from_secs(5);

/// Redacted private-save observation, independent of provider execution and review status.
#[derive(Clone, Debug)]
pub struct WorkerSaveObservation {
    /// Closed native state: waiting, saving, saved, unchanged or needs-attention.
    pub state: &'static str,
    /// When the native save state was last observed, not a filesystem event timestamp.
    pub observed_at: Option<SystemTime>,
    /// Last observed immutable private version; no review or approval is implied.
    pub version: Option<String>,
    /// Stable native issue code, never provider output or working-file content.
    pub issue: Option<&'static str>,
}

pub(super) struct ProgressOwner {
    enabled: bool,
    job: Option<(JoinHandle<Result<WorkerProgressSave, Unavailable>>, bool)>,
    next: Instant,
    final_requested: bool,
    final_completed: bool,
    observation: WorkerSaveObservation,
}
impl ProgressOwner {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            job: None,
            next: Instant::now(),
            final_requested: false,
            final_completed: false,
            observation: WorkerSaveObservation {
                state: "waiting",
                observed_at: None,
                version: None,
                issue: None,
            },
        }
    }
    pub(super) fn observation(&self) -> Option<WorkerSaveObservation> {
        self.enabled.then(|| self.observation.clone())
    }
    pub(super) fn final_observed(&self) -> bool {
        !self.enabled || self.final_completed
    }
    pub(super) fn advance(
        &mut self,
        service: &Arc<FleetService>,
        credential: &AgentCredential,
        finished: bool,
        cancelled: bool,
    ) {
        if self.job.as_ref().is_some_and(|(job, _)| job.is_finished()) {
            let (job, final_capture) = self.job.take().unwrap();
            self.observation.observed_at = Some(SystemTime::now());
            match job.join() {
                Ok(Ok(result)) => {
                    self.observation.version = Some(result.version.to_string());
                    self.observation.issue = result.issue;
                    self.observation.state = if !result.complete || result.observation_pending {
                        "needs-attention"
                    } else if result.saved_changes == 0 {
                        "unchanged"
                    } else {
                        "saved"
                    };
                }
                _ => {
                    self.observation.state = "needs-attention";
                    self.observation.issue = Some("fleet-progress-unavailable");
                }
            }
            self.next = Instant::now() + INTERVAL;
            self.final_completed |= final_capture;
        }
        if !self.enabled || cancelled || self.job.is_some() || self.final_completed {
            return;
        }
        let final_capture = finished && !self.final_requested;
        if !final_capture && (finished || Instant::now() < self.next) {
            return;
        }
        self.final_requested |= final_capture;
        self.next = Instant::now() + INTERVAL;
        self.observation.observed_at = Some(SystemTime::now());
        match service.spawn_worker_progress_save(credential) {
            Ok(job) => {
                self.job = Some((job, final_capture));
                self.observation.state = "saving";
                self.observation.issue = None;
            }
            Err(_) => {
                self.final_completed |= final_capture;
                self.observation.state = "needs-attention";
                self.observation.issue = Some("fleet-progress-start-unavailable");
            }
        }
    }
}
impl Drop for ProgressOwner {
    fn drop(&mut self) {
        if let Some((job, _)) = self.job.take() {
            let _ = job.join();
        }
    }
}
