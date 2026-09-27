//! Single-worker reconciliation for an existing project. No event stream is treated as complete.

use super::{external_store, ObservationLimits, ProjectAttachment, SavedAttachmentVersion};
use crate::ipc::Json;
use crate::root_authority::PinnedWorkspaceRoot;
use crate::CheckpointSigner;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Native scheduling policy. Scans remain bounded by the observer's separate resource limits.
#[derive(Clone, Copy, Debug)]
pub struct CaptureSchedule {
    /// Delay after a completed attempt, from 250 ms to five minutes. Signals may wake it sooner.
    pub reconciliation_interval: Duration,
    /// Per-attempt entry and byte limits; incomplete traversal never produces a saved version.
    pub limits: ObservationLimits,
}
impl Default for CaptureSchedule {
    fn default() -> Self {
        Self {
            reconciliation_interval: Duration::from_secs(5),
            limits: ObservationLimits::default(),
        }
    }
}

/// Current worker activity, separate from the last completed attempt's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePhase {
    /// Registered identities are pinned; initial history recovery has not completed.
    Starting,
    /// Reading a bounded capture of the original project.
    Scanning,
    /// Signing or writing captured bytes into external history.
    Saving,
    /// Waiting for a signal or the next reconciliation interval.
    Waiting,
    /// Stop requested; an in-flight operation may still be finishing.
    Stopping,
    /// Worker exited and will perform no further captures.
    Stopped,
    /// Worker unwound unexpectedly; no automatic replacement was launched.
    Failed,
}

/// Redacted capture health. No filesystem paths, content, signer errors or secrets are retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureOutcome {
    /// No attempt has completed in this service instance.
    Pending,
    /// New immutable history was acknowledged.
    Saved,
    /// Complete captured content already matches the saved version.
    Unchanged,
    /// The scan could not admit a complete bounded capture.
    Incomplete,
    /// The original registered source is unavailable or has changed identity.
    SourceUnavailable,
    /// The external store is unavailable or has changed identity.
    StoreUnavailable,
    /// Signing, policy checks or durable saving failed; the last acknowledged version is retained.
    SaveUnavailable,
    /// Stop was requested before this attempt acknowledged a saved capture.
    Cancelled,
}

/// Live, redacted status. Ages are measured with a monotonic clock in this process only.
#[derive(Clone, Debug)]
pub struct CaptureStatus {
    /// Monotonic status revision for waiting clients.
    pub revision: u64,
    /// Current worker activity.
    pub phase: CapturePhase,
    /// Last completed attempt; a new scan does not erase the prior result.
    pub last_outcome: CaptureOutcome,
    /// Last acknowledged/recovered immutable version, not a claim about current live bytes.
    pub saved_version: Option<SavedAttachmentVersion>,
    /// Attempts started in this service instance.
    pub attempts: u64,
    /// Newly acknowledged versions in this instance, excluding unchanged captures.
    pub versions_saved: u64,
    /// Duration of the most recent attempt, including signing and storage when attempted.
    pub last_attempt_duration: Option<Duration>,
    /// Age of the most recent completed attempt; never inferred from a stale wall-clock timestamp.
    pub last_attempt_age: Option<Duration>,
    /// Age since the start of the most recent attempt that acknowledged a complete capture.
    pub last_complete_capture_age: Option<Duration>,
}

impl CaptureStatus {
    /// Stable redacted native projection for a future UI or event subscriber. This is not a live
    /// folder snapshot: the saved identity and observation age remain explicit and independent.
    pub fn to_json(&self) -> Json {
        let phase = match self.phase {
            CapturePhase::Starting => "starting",
            CapturePhase::Scanning => "scanning",
            CapturePhase::Saving => "saving",
            CapturePhase::Waiting => "waiting",
            CapturePhase::Stopping => "stopping",
            CapturePhase::Stopped => "stopped",
            CapturePhase::Failed => "failed",
        };
        let outcome = match self.last_outcome {
            CaptureOutcome::Pending => "pending",
            CaptureOutcome::Saved => "saved",
            CaptureOutcome::Unchanged => "unchanged",
            CaptureOutcome::Incomplete => "incomplete",
            CaptureOutcome::SourceUnavailable => "source-unavailable",
            CaptureOutcome::StoreUnavailable => "store-unavailable",
            CaptureOutcome::SaveUnavailable => "save-unavailable",
            CaptureOutcome::Cancelled => "cancelled",
        };
        let millis = |duration: Option<Duration>| {
            duration.map_or(Json::Null, |duration| {
                Json::Number(duration.as_millis().min(u128::from(u64::MAX)) as u64)
            })
        };
        Json::object([
            ("schema", Json::text("mesh.attachment-capture/v1")),
            ("revision", Json::Number(self.revision)),
            ("phase", Json::text(phase)),
            ("last_outcome", Json::text(outcome)),
            (
                "saved_version",
                self.saved_version.map_or(Json::Null, |version| {
                    Json::text(version.operation().to_string())
                }),
            ),
            ("attempts", Json::Number(self.attempts)),
            ("versions_saved", Json::Number(self.versions_saved)),
            (
                "last_attempt_duration_ms",
                millis(self.last_attempt_duration),
            ),
            ("last_attempt_age_ms", millis(self.last_attempt_age)),
            (
                "last_complete_capture_age_ms",
                millis(self.last_complete_capture_age),
            ),
            ("attribution", Json::text("unknown")),
            ("atomic_snapshot", Json::Bool(false)),
        ])
    }
}

struct State {
    status: CaptureStatus,
    stop: bool,
    pending: bool,
    last_attempt: Option<Instant>,
    last_complete: Option<Instant>,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}
impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn change(&self, update: impl FnOnce(&mut State)) {
        let mut state = self.lock();
        update(&mut state);
        if state.stop
            && !matches!(
                state.status.phase,
                CapturePhase::Stopped | CapturePhase::Failed
            )
        {
            state.status.phase = CapturePhase::Stopping;
        }
        state.status.revision = state.status.revision.saturating_add(1);
        self.changed.notify_all();
    }
    fn stopped(&self) -> bool {
        self.lock().stop
    }
    fn snapshot(state: &State) -> CaptureStatus {
        let mut status = state.status.clone();
        status.last_attempt_age = state.last_attempt.map(|time| time.elapsed());
        status.last_complete_capture_age = state.last_complete.map(|time| time.elapsed());
        status
    }
}

/// One native background controller. Starting it explicitly authorizes captures with the supplied
/// host signer. It provisions no agent and never writes, locks or takes over the original project.
/// Dropping requests stop without blocking; use `stop_and_join` to wait for confirmed termination.
pub struct AttachmentCaptureService {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
impl AttachmentCaptureService {
    /// Pin an existing registration and its external metadata, then start one bounded worker.
    /// The host owns key setup and lifetime. No key is generated or persisted by this service.
    pub fn start(
        metadata: &Path,
        signer: Arc<dyn CheckpointSigner>,
        schedule: CaptureSchedule,
    ) -> io::Result<Self> {
        let attachment = ProjectAttachment::reopen(metadata)?;
        let store = external_store(metadata, &attachment)?;
        Self::start_pinned(metadata, attachment, store, signer, schedule)
    }

    pub(super) fn start_pinned(
        metadata: &Path,
        attachment: ProjectAttachment,
        store: PinnedWorkspaceRoot,
        signer: Arc<dyn CheckpointSigner>,
        schedule: CaptureSchedule,
    ) -> io::Result<Self> {
        attachment.ensure_current()?;
        store.ensure_namespace_identity()?;
        super::detachment::ensure_attached(&store)?;
        schedule.limits.validate()?;
        if !(Duration::from_millis(250)..=Duration::from_secs(300))
            .contains(&schedule.reconciliation_interval)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "capture interval must be between 250 ms and five minutes",
            ));
        }
        let metadata = metadata.to_path_buf();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                status: CaptureStatus {
                    revision: 0,
                    phase: CapturePhase::Starting,
                    last_outcome: CaptureOutcome::Pending,
                    saved_version: None,
                    attempts: 0,
                    versions_saved: 0,
                    last_attempt_duration: None,
                    last_attempt_age: None,
                    last_complete_capture_age: None,
                },
                stop: false,
                pending: true,
                last_attempt: None,
                last_complete: None,
            }),
            changed: Condvar::new(),
        });
        let running = shared.clone();
        let worker = thread::Builder::new()
            .name("mesh-attachment-capture".to_owned())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(&running, attachment, store, metadata, signer, schedule);
                }));
                running.change(|state| {
                    state.status.phase = if result.is_ok() {
                        CapturePhase::Stopped
                    } else {
                        CapturePhase::Failed
                    }
                });
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    /// A redacted view of current activity and the last known saved point.
    pub fn status(&self) -> CaptureStatus {
        Shared::snapshot(&self.shared.lock())
    }

    /// Coalesce a missed-event or explicit checkpoint signal into at most one pending rescan.
    /// Returns false after stop is requested. No source path or file bytes are accepted here.
    pub fn request_capture(&self) -> bool {
        let mut state = self.shared.lock();
        if state.stop
            || matches!(
                state.status.phase,
                CapturePhase::Stopped | CapturePhase::Failed
            )
        {
            return false;
        }
        state.pending = true;
        self.shared.changed.notify_all();
        true
    }

    /// Wait for a status revision or timeout. Spurious wakes do not masquerade as progress.
    pub fn wait_for_update(&self, revision: u64, timeout: Duration) -> CaptureStatus {
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(self.shared.lock(), timeout, |state| {
                state.status.revision <= revision
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Shared::snapshot(&state)
    }

    /// Stop future scans and wake a waiting worker. An in-flight durable commit may still finish.
    pub fn request_stop(&self) {
        self.shared.change(|state| {
            state.stop = true;
            state.pending = false;
        });
    }

    /// Wait for the current bounded attempt to exit. Slow filesystem I/O or an external signer can
    /// delay this wait; stop does not claim to interrupt those operations or roll back a commit.
    pub fn stop_and_join(mut self) -> io::Result<CaptureStatus> {
        self.request_stop();
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("attachment capture worker failed"))?;
        }
        Ok(self.status())
    }
}
impl Drop for AttachmentCaptureService {
    fn drop(&mut self) {
        self.request_stop();
    }
}

fn run(
    shared: &Shared,
    attachment: ProjectAttachment,
    store: PinnedWorkspaceRoot,
    metadata: PathBuf,
    signer: Arc<dyn CheckpointSigner>,
    schedule: CaptureSchedule,
) {
    if shared.stopped() {
        return;
    }
    let actor = signer.public_key();
    if store
        .filesystem()
        .inspect_entry(Path::new(super::history::HISTORY))
        .is_ok()
    {
        match attachment.saved_versions_in_store(&metadata, store.clone()) {
            Ok(versions) => {
                shared.change(|state| state.status.saved_version = versions.last().copied())
            }
            Err(_) => {
                shared.change(|state| state.status.last_outcome = CaptureOutcome::SaveUnavailable)
            }
        }
    }
    loop {
        {
            let mut state = shared.lock();
            if !state.stop && !state.pending {
                let (next, _) = shared
                    .changed
                    .wait_timeout_while(state, schedule.reconciliation_interval, |state| {
                        !state.stop && !state.pending
                    })
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state = next;
            }
            if state.stop {
                return;
            }
            state.pending = false;
        }
        let started = Instant::now();
        shared.change(|state| {
            state.status.phase = CapturePhase::Scanning;
            state.status.attempts = state.status.attempts.saturating_add(1);
        });
        let mut saved = None;
        let outcome = if store.ensure_namespace_identity().is_err() {
            CaptureOutcome::StoreUnavailable
        } else if attachment.ensure_current().is_err() {
            CaptureOutcome::SourceUnavailable
        } else {
            match attachment.capture_inputs(schedule.limits) {
                Err(_) => CaptureOutcome::Incomplete,
                Ok(input) => {
                    if shared.stopped() {
                        complete_attempt(shared, started, CaptureOutcome::Cancelled, None);
                        return;
                    }
                    shared.change(|state| state.status.phase = CapturePhase::Saving);
                    let result = attachment.save_capture_in_store(
                        &metadata,
                        &input,
                        actor,
                        |payload| {
                            if shared.stopped() {
                                return Err("capture stopped".to_owned());
                            }
                            let signature = signer.sign(payload)?;
                            if shared.stopped() {
                                return Err("capture stopped".to_owned());
                            }
                            Ok(signature)
                        },
                        store.clone(),
                    );
                    match result {
                        Ok(version) => {
                            saved = Some(version);
                            if shared.lock().status.saved_version == saved {
                                CaptureOutcome::Unchanged
                            } else {
                                CaptureOutcome::Saved
                            }
                        }
                        Err(_) if shared.stopped() => CaptureOutcome::Cancelled,
                        Err(_) => CaptureOutcome::SaveUnavailable,
                    }
                }
            }
        };
        complete_attempt(shared, started, outcome, saved);
    }
}

fn complete_attempt(
    shared: &Shared,
    started: Instant,
    outcome: CaptureOutcome,
    saved: Option<SavedAttachmentVersion>,
) {
    shared.change(|state| {
        state.status.phase = CapturePhase::Waiting;
        state.status.last_outcome = outcome;
        state.status.last_attempt_duration = Some(started.elapsed());
        state.last_attempt = Some(Instant::now());
        if let Some(saved) = saved {
            state.status.saved_version = Some(saved);
            state.last_complete = Some(started);
            if outcome == CaptureOutcome::Saved {
                state.status.versions_saved = state.status.versions_saved.saturating_add(1);
            }
        }
    });
}
