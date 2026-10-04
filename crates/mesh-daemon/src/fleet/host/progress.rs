use super::super::service::WorkerProgressSave;
use super::*;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
const INTERVAL: Duration = Duration::from_secs(5);
const FINAL_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const MAX_FINAL_ATTEMPTS: u8 = 3;

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
    final_attempts: u8,
    final_completed: bool,
    observation: WorkerSaveObservation,
}
impl ProgressOwner {
    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            job: None,
            next: Instant::now(),
            final_attempts: 0,
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
        self.advance_with(Instant::now(), finished, cancelled, || {
            service.spawn_worker_progress_save(credential)
        });
    }
    fn advance_with(
        &mut self,
        now: Instant,
        finished: bool,
        cancelled: bool,
        spawn: impl FnOnce() -> std::io::Result<JoinHandle<Result<WorkerProgressSave, Unavailable>>>,
    ) {
        if self.job.as_ref().is_some_and(|(job, _)| job.is_finished()) {
            let (job, final_capture) = self.job.take().unwrap();
            self.observation.observed_at = Some(SystemTime::now());
            let mut retry_observation = false;
            match job.join() {
                Ok(Ok(result)) => {
                    // Retry only a complete native save whose fleet acknowledgment is pending.
                    // No process retry, authority renewal or unsupported-file repair is implied.
                    retry_observation = result.complete && result.observation_pending;
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
            let retry_final =
                final_capture && retry_observation && self.final_attempts < MAX_FINAL_ATTEMPTS;
            self.next = now
                + if retry_final {
                    FINAL_RETRY_INTERVAL
                } else {
                    INTERVAL
                };
            self.final_completed |= final_capture && !retry_final;
        }
        if !self.enabled || cancelled || self.job.is_some() || self.final_completed {
            return;
        }
        let final_capture = finished;
        if (!final_capture && self.final_attempts > 0)
            || ((self.final_attempts > 0 || !final_capture) && now < self.next)
        {
            return;
        }
        if final_capture {
            self.final_attempts += 1;
        }
        self.next = now + INTERVAL;
        self.observation.observed_at = Some(SystemTime::now());
        match spawn() {
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

#[cfg(test)]
mod tests;
